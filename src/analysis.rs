use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use tree_sitter::{Language, Node, Parser};

#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub identity: String,
    pub line: usize,
    pub column: usize,
    pub column_utf16: usize,
    pub end_line: usize,
    /// Zero-based (row, UTF-8 byte column) node span. Unlike byte offsets, these
    /// are unchanged by the separator repairs applied before parsing.
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub complexity: u32,
    /// Owned statement lines, sorted and deduplicated.
    pub lines: Vec<usize>,
}

fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}

// Flags for the node kinds the analyzer matches. `kind_flags` holds the only
// kind-string matches; `Kinds` evaluates them once per grammar symbol id.
const FUNCTION: u16 = 1 << 0;
const TYPE_ONLY: u16 = 1 << 1;
const DECISION: u16 = 1 << 2;
const LOGICAL: u16 = 1 << 3;
const OWNED_LINE: u16 = 1 << 4;
const PARAMETER: u16 = 1 << 5;
const PATTERN: u16 = 1 << 6;
const CLASS: u16 = 1 << 7;
const COMMENT: u16 = 1 << 8;
const STATEMENT_BLOCK: u16 = 1 << 9;
const NAMING_PARENT: u16 = 1 << 10;
const ASSIGNMENT_PARENT: u16 = 1 << 11;

fn kind_flags(kind: &str) -> u16 {
    let mut flags = 0;
    if is_function(kind) {
        flags |= FUNCTION;
    }
    // Type-only branches such as conditional types do not affect runtime flow.
    // "return_type" is a field name, not a kind, so it never matches; matching
    // the field instead would change complexity and owned lines.
    if matches!(kind, "type_annotation" | "type_parameters" | "return_type") {
        flags |= TYPE_ONLY;
    }
    if matches!(
        kind,
        "if_statement"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "catch_clause"
            | "switch_case"
            | "ternary_expression"
    ) {
        flags |= DECISION;
    }
    if matches!(
        kind,
        "binary_expression" | "augmented_assignment_expression"
    ) {
        flags |= LOGICAL;
    }
    if matches!(
        kind,
        "return_statement"
            | "throw_statement"
            | "expression_statement"
            | "lexical_declaration"
            | "variable_declaration"
            | "if_statement"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "break_statement"
            | "continue_statement"
    ) {
        flags |= OWNED_LINE;
    }
    if matches!(kind, "required_parameter" | "optional_parameter") {
        flags |= PARAMETER;
    }
    if matches!(kind, "assignment_pattern" | "object_assignment_pattern") {
        flags |= PATTERN;
    }
    if matches!(
        kind,
        "class_declaration" | "abstract_class_declaration" | "class"
    ) {
        flags |= CLASS;
    }
    if kind == "comment" {
        flags |= COMMENT;
    }
    if kind == "statement_block" {
        flags |= STATEMENT_BLOCK;
    }
    if matches!(
        kind,
        "variable_declarator" | "public_field_definition" | "pair"
    ) {
        flags |= NAMING_PARENT;
    }
    if kind == "assignment_expression" {
        flags |= ASSIGNMENT_PARENT;
    }
    flags
}

/// Field ids used by the analyzer. `None` when a grammar lacks the field, which
/// `child_by_field_name` would equally treat as no child.
#[derive(Debug, Clone, Copy)]
struct Fields {
    body: Option<u16>,
    key: Option<u16>,
    left: Option<u16>,
    name: Option<u16>,
    operator: Option<u16>,
    parameters: Option<u16>,
    right: Option<u16>,
    value: Option<u16>,
}

/// Per-grammar kind flags and names, indexed by symbol id. A kind can have
/// several ids (named, anonymous, aliased), so every id is evaluated by its
/// kind string; ids outside the table, such as ERROR's 65535, have no flags.
#[derive(Debug)]
struct Kinds {
    flags: Vec<u16>,
    names: Vec<Option<Box<str>>>,
    fields: Fields,
}

impl Kinds {
    fn new(language: &Language) -> Self {
        let names: Vec<Option<Box<str>>> = (0..language.node_kind_count())
            .map(|id| {
                u16::try_from(id)
                    .ok()
                    .and_then(|id| language.node_kind_for_id(id))
                    .map(Box::from)
            })
            .collect();
        let flags = names
            .iter()
            .map(|kind| kind.as_deref().map_or(0, kind_flags))
            .collect();
        let field = |name: &str| language.field_id_for_name(name).map(|id| id.get());
        Self {
            flags,
            names,
            fields: Fields {
                body: field("body"),
                key: field("key"),
                left: field("left"),
                name: field("name"),
                operator: field("operator"),
                parameters: field("parameters"),
                right: field("right"),
                value: field("value"),
            },
        }
    }

    fn flags(&self, node: Node<'_>) -> u16 {
        self.flags
            .get(usize::from(node.kind_id()))
            .copied()
            .unwrap_or(0)
    }

    /// The kind string hashed into identities, without a symbol-name lookup.
    fn kind<'a>(&'a self, node: Node<'a>) -> &'a str {
        match self.names.get(usize::from(node.kind_id())) {
            Some(Some(kind)) => kind,
            _ => node.kind(),
        }
    }

    fn child<'t>(node: Node<'t>, field: Option<u16>) -> Option<Node<'t>> {
        field.and_then(|field| node.child_by_field_id(field))
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn skip_type_trivia(mut source: &str) -> &str {
    loop {
        source = source.trim_start_matches(char::is_whitespace);
        if source.starts_with("//") {
            if let Some((_, rest)) = source.split_once('\n') {
                source = rest;
                continue;
            }
        } else if let Some(comment) = source.strip_prefix("/*")
            && let Some((_, rest)) = comment.split_once("*/")
        {
            source = rest;
            continue;
        }
        return source;
    }
}

fn local_name(node: Node<'_>, parent: Option<Node<'_>>, source: &str, kinds: &Kinds) -> String {
    let fields = kinds.fields;
    if let Some(name) = Kinds::child(node, fields.name) {
        return text(name, source).into();
    }
    if let Some(parent) = parent {
        let flags = kinds.flags(parent);
        if flags & NAMING_PARENT != 0 {
            if let Some(name) =
                Kinds::child(parent, fields.name).or_else(|| Kinds::child(parent, fields.key))
            {
                return text(name, source).into();
            }
        } else if flags & ASSIGNMENT_PARENT != 0
            && let Some(left) = Kinds::child(parent, fields.left)
        {
            return text(left, source).into();
        }
    }
    format!(
        "<anonymous@{}:{}>",
        node.start_position().row + 1,
        node.start_position().column + 1
    )
}

#[derive(Debug)]
enum ScopeName<'a> {
    Class(&'a str),
    // Reuse the emitted name rather than keeping another full prefix per scope.
    Function(usize),
}

#[derive(Debug)]
struct Scope<'a> {
    end: usize,
    name: ScopeName<'a>,
}

/// Preorder function starts are monotonic. Count each prefix segment once,
/// resetting at a new line; storage is constant rather than a per-byte index.
#[derive(Debug, Default)]
struct Utf16Column {
    line_start: usize,
    byte: usize,
    units: usize,
}

impl Utf16Column {
    fn at(&mut self, node: Node<'_>, source: &str) -> usize {
        let start = node.start_byte();
        let line_start = start - node.start_position().column;
        if self.line_start != line_start {
            self.line_start = line_start;
            self.byte = line_start;
            self.units = 0;
        }
        self.units += source[self.byte..start].encode_utf16().count();
        self.byte = start;
        self.units + 1
    }
}

// TreeCursor owns its traversal stack on the heap. Returning false prunes a
// subtree, so nested functions and type-only syntax retain their own ownership.
fn walk(node: Node<'_>, mut visit: impl FnMut(Node<'_>) -> bool) {
    let mut cursor = node.walk();
    loop {
        if visit(cursor.node()) && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

// The same preorder traversal as `walk`, also passing the visited node's
// ancestors up to the walk's starting node (outermost first); called on a
// tree's root, that is the `Node::parent` chain. `Node::parent` searches down
// from the root on every call; the cursor already knows the path.
fn walk_with_ancestors<'t>(node: Node<'t>, mut visit: impl FnMut(Node<'t>, &[Node<'t>]) -> bool) {
    let mut cursor = node.walk();
    let mut ancestors = Vec::new();
    loop {
        let current = cursor.node();
        if visit(current, &ancestors) && cursor.goto_first_child() {
            ancestors.push(current);
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            ancestors.pop();
        }
    }
}

// Each decision adds one independent execution path. Descendant functions are
// scored separately, never charged to their enclosing function. Owned lines are
// pushed unsorted; the caller sorts and deduplicates them once.
fn metrics(
    node: Node<'_>,
    source: &str,
    kinds: &Kinds,
    complexity: &mut u32,
    lines: &mut Vec<usize>,
) {
    let fields = kinds.fields;
    walk(node, |node| {
        let flags = kinds.flags(node);
        if flags & (FUNCTION | TYPE_ONLY) != 0 {
            return false;
        }
        if flags & DECISION != 0 {
            *complexity += 1;
        }
        if flags & LOGICAL != 0
            && let Some(operator) = Kinds::child(node, fields.operator)
            && matches!(
                text(operator, source),
                "&&" | "||" | "??" | "&&=" | "||=" | "??="
            )
        {
            *complexity += 1;
        }
        if flags & OWNED_LINE != 0 {
            lines.push(node.start_position().row + 1);
        }
        let initializer = if flags & PARAMETER != 0 {
            Kinds::child(node, fields.value)
        } else if flags & PATTERN != 0 {
            Kinds::child(node, fields.right)
        } else {
            None
        };
        if let Some(value) = initializer {
            lines.push(value.start_position().row + 1);
        }
        true
    });
}

// Hashed tokens are buffered and fed to SHA-256 in chunks: the digest of a
// stream does not depend on how it is split.
const HASH_CHUNK: usize = 16 * 1024;

fn collect(node: Node<'_>, source: &str, kinds: &Kinds, out: &mut Vec<Method>) {
    let fields = kinds.fields;
    let mut scopes: Vec<Scope<'_>> = Vec::new();
    let mut utf16 = Utf16Column::default();
    let mut buffer = Vec::with_capacity(HASH_CHUNK);
    walk_with_ancestors(node, |node, ancestors| {
        let flags = kinds.flags(node);
        let class_name = if flags & CLASS != 0 {
            Kinds::child(node, fields.name)
        } else {
            None
        };
        if flags & FUNCTION == 0 && class_name.is_none() {
            return true;
        }
        // Only scope events need to expire entries; intervening AST nodes do not
        // use the stack. Named classes reset qualification; anonymous ones don't.
        while scopes
            .last()
            .is_some_and(|scope| scope.end <= node.start_byte())
        {
            scopes.pop();
        }
        if let Some(class_name) = class_name {
            scopes.push(Scope {
                end: node.end_byte(),
                name: ScopeName::Class(text(class_name, source)),
            });
        } else if let Some(body) = Kinds::child(node, fields.body) {
            let local = local_name(node, ancestors.last().copied(), source, kinds);
            let name = match scopes.last().map(|scope| &scope.name) {
                Some(ScopeName::Class(class)) => format!("{class}::{local}"),
                Some(ScopeName::Function(index)) => format!("{}::{local}", out[*index].name),
                None => local,
            };
            let mut complexity = 1;
            let mut lines = Vec::new();
            metrics(body, source, kinds, &mut complexity, &mut lines);
            // Expression-bodied arrows have no statement node.
            if kinds.flags(body) & STATEMENT_BLOCK == 0 {
                lines.push(body.start_position().row + 1);
            }
            // Default argument expressions execute as part of the function.
            if let Some(parameters) = Kinds::child(node, fields.parameters) {
                metrics(parameters, source, kinds, &mut complexity, &mut lines);
            }
            lines.sort_unstable();
            lines.dedup();
            // Hash owned tokens, excluding trivia and descendant functions (which
            // have their own identities). Pruning avoids quadratic hashing of
            // deeply nested functions; line shifts do not change identity.
            let mut hash = Sha256::new();
            buffer.clear();
            walk(node, |token| {
                let flags = kinds.flags(token);
                if flags & COMMENT != 0 || (token != node && flags & FUNCTION != 0) {
                    return false;
                }
                if token.child_count() == 0 {
                    buffer.extend_from_slice(kinds.kind(token).as_bytes());
                    buffer.push(0);
                    buffer.extend_from_slice(text(token, source).as_bytes());
                    buffer.push(0);
                    if buffer.len() >= HASH_CHUNK {
                        hash.update(&buffer);
                        buffer.clear();
                    }
                }
                true
            });
            hash.update(&buffer);
            out.push(Method {
                identity: hex::encode(hash.finalize()),
                name,
                line: node.start_position().row + 1,
                column: node.start_position().column + 1,
                column_utf16: utf16.at(node, source),
                end_line: node.end_position().row + 1,
                start: (node.start_position().row, node.start_position().column),
                end: (node.end_position().row, node.end_position().column),
                complexity,
                lines,
            });
            scopes.push(Scope {
                end: node.end_byte(),
                name: ScopeName::Function(out.len() - 1),
            });
        }
        true
    });
}

/// One parser per grammar, reused for all files in one scan. Trees are never
/// reused between sources, so no stale incremental parse state is retained.
#[derive(Default)]
pub struct Analyzer {
    typescript: Option<Grammar>,
    tsx: Option<Grammar>,
}

/// A parser and the kind tables of the same language, created together so a
/// tree is always read with its own grammar's symbol ids.
struct Grammar {
    parser: Parser,
    kinds: Kinds,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyzer")
            .field("typescript_initialized", &self.typescript.is_some())
            .field("tsx_initialized", &self.tsx.is_some())
            .finish()
    }
}

/// The first syntax error in a source. Displays the message `analyze` has always used.
#[derive(Debug)]
pub(crate) struct SyntaxError {
    pub path: String,
    pub line: usize,
    /// One-based UTF-8 byte column.
    pub column: usize,
    pub column_utf16: usize,
    pub snippet: String,
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid TypeScript syntax in {}:{}:{} (UTF-8 byte column): {}",
            self.path, self.line, self.column, self.snippet
        )
    }
}

impl std::error::Error for SyntaxError {}

impl Analyzer {
    pub fn analyze(&mut self, path: &str, source: &str) -> Result<Vec<Method>> {
        let tsx = path.ends_with(".tsx");
        let slot = if tsx {
            &mut self.tsx
        } else {
            &mut self.typescript
        };
        if slot.is_none() {
            let mut parser = Parser::new();
            let language: Language = if tsx {
                tree_sitter_typescript::LANGUAGE_TSX
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT
            }
            .into();
            parser
                .set_language(&language)
                .context("cannot initialize TypeScript parser")?;
            *slot = Some(Grammar {
                parser,
                kinds: Kinds::new(&language),
            });
        }
        let Grammar { parser, kinds } = slot.as_mut().context("TypeScript parser unavailable")?;
        let mut parsed_source = Cow::Borrowed(source);
        let mut tree = parser
            .parse(source, None)
            .context("TypeScript parser failed")?;
        // The grammar can end an interface/object type early when a return type is
        // followed by another generic call signature without an explicit ';'.
        // Inserting that optional separator at an existing line boundary leaves
        // every runtime line/column and original coverage hash unchanged. Never
        // ignore an ERROR or synthesize braces; the repaired parse must be valid.
        for _ in 0..64 {
            if !tree.root_node().has_error() {
                break;
            }
            let mut separators = Vec::new();
            let mut type_exports = Vec::new();
            let mut token_end = 0;
            walk_with_ancestors(tree.root_node(), |node, ancestors| {
                let parent = ancestors.last().copied();
                // TypeScript 5's `export type *` is parsed as an otherwise
                // complete export with one ERROR containing only `type`.
                // Erasing that type-only modifier preserves byte coordinates.
                if node.is_error()
                    && text(node, &parsed_source) == "type"
                    && let Some(parent) = parent
                    && parent.kind() == "export_statement"
                    && parent
                        .child_by_field_name("source")
                        .is_some_and(|source| !source.has_error())
                {
                    let mut previous = node.prev_sibling();
                    while previous.is_some_and(|sibling| sibling.kind() == "comment") {
                        previous = previous.and_then(|sibling| sibling.prev_sibling());
                    }
                    let mut next = node.next_sibling();
                    while next.is_some_and(|sibling| sibling.kind() == "comment") {
                        next = next.and_then(|sibling| sibling.next_sibling());
                    }
                    if previous.is_some_and(|sibling| sibling.kind() == "export")
                        && next.is_some_and(|sibling| {
                            matches!(sibling.kind(), "*" | "namespace_export")
                                && !sibling.has_error()
                        })
                    {
                        type_exports.push(node.byte_range());
                    }
                }
                if node.kind() == "type_parameters"
                    && parent.is_some_and(|parent| parent.is_error())
                    && let Some(parameters) = node.next_named_sibling()
                    && parameters.kind() == "formal_parameters"
                    && !parameters.has_error()
                    && skip_type_trivia(&parsed_source[parameters.end_byte()..]).starts_with(':')
                {
                    let in_type_body = ancestors.iter().rev().any(|ancestor| {
                        matches!(ancestor.kind(), "interface_body" | "object_type")
                    });
                    let previous = &parsed_source[..token_end];
                    let gap = &parsed_source[token_end..node.start_byte()];
                    if in_type_body
                        && gap.contains('\n')
                        && skip_type_trivia(gap).is_empty()
                        && !previous.ends_with(['{', ';', ','])
                    {
                        separators.push(token_end);
                    }
                }
                if node.is_missing()
                    && node.kind() == "}"
                    && parent.is_some_and(|parent| {
                        matches!(parent.kind(), "interface_body" | "object_type")
                    })
                {
                    let mut previous = node.prev_named_sibling();
                    while previous.is_some_and(|sibling| sibling.kind() == "comment") {
                        previous = previous.and_then(|sibling| sibling.prev_named_sibling());
                    }
                    if let Some(previous) = previous
                        && previous.kind() == "call_signature"
                        && !previous.has_error()
                    {
                        let remainder = &parsed_source[previous.end_byte()..];
                        let next = skip_type_trivia(remainder);
                        let gap = &remainder[..remainder.len() - next.len()];
                        if gap.contains('\n') && next.starts_with('<') {
                            separators.push(previous.end_byte());
                        }
                    }
                }
                if node.child_count() == 0 && !node.is_missing() && node.kind() != "comment" {
                    token_end = node.end_byte();
                }
                true
            });
            if separators.is_empty() && type_exports.is_empty() {
                break;
            }
            for range in type_exports {
                parsed_source.to_mut().replace_range(range, "    ");
            }
            separators.sort_unstable();
            separators.dedup();
            for byte in separators.into_iter().rev() {
                parsed_source.to_mut().insert(byte, ';');
            }
            tree = parser
                .parse(parsed_source.as_ref(), None)
                .context("TypeScript parser failed")?;
        }
        let source = parsed_source.as_ref();
        if tree.root_node().has_error() {
            let mut issue = None;
            // Descend to the earliest concrete ERROR or missing token using the
            // same bounded-stack traversal as metrics, including malformed input.
            walk(tree.root_node(), |node| {
                if issue.is_some() {
                    return false;
                }
                if node.is_error() || node.is_missing() {
                    issue = Some(node.start_position());
                    return false;
                }
                node.has_error()
            });
            let position = issue.unwrap_or_else(|| tree.root_node().start_position());
            let line = source.lines().nth(position.row).unwrap_or("");
            let snippet: String = line.chars().take(160).collect();
            // UTF-16 units of the characters that start before the byte column.
            let column_utf16 = line
                .char_indices()
                .take_while(|&(start, _)| start < position.column)
                .map(|(_, c)| c.len_utf16())
                .sum::<usize>()
                + 1;
            return Err(SyntaxError {
                path: path.to_owned(),
                line: position.row + 1,
                column: position.column + 1,
                column_utf16,
                snippet,
            }
            .into());
        }
        let mut out = Vec::new();
        collect(tree.root_node(), source, kinds, &mut out);
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
