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
// Hashed tokens are buffered and fed to SHA-256 in chunks: the digest of a
// stream does not depend on how it is split.
const HASH_CHUNK: usize = 16 * 1024;

/// An open function with a body: the state of its identity hash and metrics,
/// finished when the traversal leaves the function node. Nodes are classified
/// by depth, never by byte range: zero-width MISSING nodes share positions.
struct Frame<'t> {
    /// Index of the function's `Method` in the output.
    index: usize,
    /// Depth of the function node in the traversal.
    depth: usize,
    body: Node<'t>,
    parameters: Option<Node<'t>>,
    hash: Sha256,
    buffer: Vec<u8>,
    complexity: u32,
    lines: Vec<usize>,
    /// Depth of the comment or descendant function whose subtree is not hashed.
    hash_pruned: Option<usize>,
    /// Depth of the `body` or `parameters` node the metrics walk is inside.
    metrics_root: Option<usize>,
    /// Depth of the function or type-only node whose subtree is not measured.
    metrics_pruned: Option<usize>,
}

impl Frame<'_> {
    /// Account for a strict descendant of the function node, in preorder. Hash
    /// and metrics pruning are independent of each other.
    fn observe(&mut self, node: Node<'_>, flags: u16, depth: usize, source: &str, kinds: &Kinds) {
        // Hash owned tokens, excluding trivia and descendant functions (which
        // have their own identities). Pruning avoids quadratic hashing of
        // deeply nested functions; line shifts do not change identity.
        if self.hash_pruned.is_none() {
            if flags & (COMMENT | FUNCTION) != 0 {
                self.hash_pruned = Some(depth);
            } else if node.child_count() == 0 {
                self.buffer.extend_from_slice(kinds.kind(node).as_bytes());
                self.buffer.push(0);
                self.buffer.extend_from_slice(text(node, source).as_bytes());
                self.buffer.push(0);
                if self.buffer.len() >= HASH_CHUNK {
                    self.hash.update(&self.buffer);
                    self.buffer.clear();
                }
            }
        }
        // Only the body and the parameters (default argument expressions execute
        // as part of the function) are measured; both are direct children.
        if self.metrics_root.is_none()
            && depth == self.depth + 1
            && (node == self.body || Some(node) == self.parameters)
        {
            self.metrics_root = Some(depth);
        }
        if self.metrics_root.is_none() || self.metrics_pruned.is_some() {
            return;
        }
        if flags & (FUNCTION | TYPE_ONLY) != 0 {
            self.metrics_pruned = Some(depth);
            return;
        }
        let fields = kinds.fields;
        if flags & DECISION != 0 {
            self.complexity += 1;
        }
        if flags & LOGICAL != 0
            && let Some(operator) = Kinds::child(node, fields.operator)
            && matches!(
                text(operator, source),
                "&&" | "||" | "??" | "&&=" | "||=" | "??="
            )
        {
            self.complexity += 1;
        }
        if flags & OWNED_LINE != 0 {
            self.lines.push(node.start_position().row + 1);
        }
        let initializer = if flags & PARAMETER != 0 {
            Kinds::child(node, fields.value)
        } else if flags & PATTERN != 0 {
            Kinds::child(node, fields.right)
        } else {
            None
        };
        if let Some(value) = initializer {
            self.lines.push(value.start_position().row + 1);
        }
    }

    /// The traversal left a descendant at `depth`; end any pruning it started.
    fn leave(&mut self, depth: usize) {
        if self.hash_pruned == Some(depth) {
            self.hash_pruned = None;
        }
        if self.metrics_pruned == Some(depth) {
            self.metrics_pruned = None;
        }
        if self.metrics_root == Some(depth) {
            self.metrics_root = None;
        }
    }
}

struct Collector<'a, 't> {
    source: &'a str,
    kinds: &'a Kinds,
    out: &'a mut Vec<Method>,
    scopes: Vec<Scope<'a>>,
    utf16: Utf16Column,
    /// Open functions, innermost last. Every open function node is an ancestor
    /// of the current node, so only the innermost frame can own the current
    /// node: a nested function prunes it from the frames around it.
    frames: Vec<Frame<'t>>,
    /// Hash buffers of finished frames, reused by later ones.
    spare: Vec<Vec<u8>>,
}

impl<'t> Collector<'_, 't> {
    fn visit(&mut self, node: Node<'t>, parent: Option<Node<'t>>, depth: usize) {
        let (source, kinds) = (self.source, self.kinds);
        let fields = kinds.fields;
        let flags = kinds.flags(node);
        if let Some(frame) = self.frames.last_mut() {
            frame.observe(node, flags, depth, source, kinds);
        }
        let class_name = if flags & CLASS != 0 {
            Kinds::child(node, fields.name)
        } else {
            None
        };
        if flags & FUNCTION == 0 && class_name.is_none() {
            return;
        }
        // Only scope events need to expire entries; intervening AST nodes do not
        // use the stack. Named classes reset qualification; anonymous ones don't.
        while self
            .scopes
            .last()
            .is_some_and(|scope| scope.end <= node.start_byte())
        {
            self.scopes.pop();
        }
        if let Some(class_name) = class_name {
            self.scopes.push(Scope {
                end: node.end_byte(),
                name: ScopeName::Class(text(class_name, source)),
            });
        } else if let Some(body) = Kinds::child(node, fields.body) {
            let local = local_name(node, parent, source, kinds);
            let name = match self.scopes.last().map(|scope| &scope.name) {
                Some(ScopeName::Class(class)) => format!("{class}::{local}"),
                Some(ScopeName::Function(index)) => format!("{}::{local}", self.out[*index].name),
                None => local,
            };
            // Identity, complexity and lines are filled in when the function is left.
            self.out.push(Method {
                identity: String::new(),
                name,
                line: node.start_position().row + 1,
                column: node.start_position().column + 1,
                column_utf16: self.utf16.at(node, source),
                end_line: node.end_position().row + 1,
                complexity: 1,
                lines: Vec::new(),
            });
            let index = self.out.len() - 1;
            self.scopes.push(Scope {
                end: node.end_byte(),
                name: ScopeName::Function(index),
            });
            let mut lines = Vec::new();
            // Expression-bodied arrows have no statement node.
            if kinds.flags(body) & STATEMENT_BLOCK == 0 {
                lines.push(body.start_position().row + 1);
            }
            let mut buffer = self
                .spare
                .pop()
                .unwrap_or_else(|| Vec::with_capacity(HASH_CHUNK));
            buffer.clear();
            // The function node has children (its body), so it is never a hashed token.
            self.frames.push(Frame {
                index,
                depth,
                body,
                parameters: Kinds::child(node, fields.parameters),
                hash: Sha256::new(),
                buffer,
                complexity: 1,
                lines,
                hash_pruned: None,
                metrics_root: None,
                metrics_pruned: None,
            });
        }
    }

    fn leave(&mut self, depth: usize) {
        if let Some(mut frame) = self.frames.pop_if(|frame| frame.depth == depth) {
            frame.hash.update(&frame.buffer);
            frame.buffer.clear();
            frame.lines.sort_unstable();
            frame.lines.dedup();
            let method = &mut self.out[frame.index];
            method.identity = hex::encode(frame.hash.finalize());
            method.complexity = frame.complexity;
            method.lines = frame.lines;
            self.spare.push(frame.buffer);
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.leave(depth);
        }
    }
}

// One preorder traversal does the work of the outer scope walk, every
// function's metrics walk and every function's token hash walk: each node is
// visited once and credited to the innermost open function. Never prunes.
fn collect<'t>(node: Node<'t>, source: &str, kinds: &Kinds, out: &mut Vec<Method>) {
    let mut collector = Collector {
        source,
        kinds,
        out,
        scopes: Vec::new(),
        utf16: Utf16Column::default(),
        frames: Vec::new(),
        spare: Vec::new(),
    };
    let mut cursor = node.walk();
    let mut ancestors: Vec<Node<'t>> = Vec::new();
    loop {
        let current = cursor.node();
        collector.visit(current, ancestors.last().copied(), ancestors.len());
        if cursor.goto_first_child() {
            ancestors.push(current);
            continue;
        }
        collector.leave(ancestors.len());
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            ancestors.pop();
            collector.leave(ancestors.len());
        }
    }
}
// Private copies of `walk` and `walk_with_ancestors` for the oracle, so a
// broken production traversal (or a mutant of one) makes the oracle tests
// fail instead of hanging them.
#[cfg(test)]
fn oracle_walk(node: Node<'_>, mut visit: impl FnMut(Node<'_>) -> bool) {
    oracle_walk_with_ancestors(node, |node, _| visit(node));
}

#[cfg(test)]
fn oracle_walk_with_ancestors<'t>(
    node: Node<'t>,
    mut visit: impl FnMut(Node<'t>, &[Node<'t>]) -> bool,
) {
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

// Reference implementation kept for tests: the production `collect` computes
// the same values in a single traversal.
// Each decision adds one independent execution path. Descendant functions are
// scored separately, never charged to their enclosing function. Owned lines are
// pushed unsorted; the caller sorts and deduplicates them once.
#[cfg(test)]
fn metrics_oracle(
    node: Node<'_>,
    source: &str,
    kinds: &Kinds,
    complexity: &mut u32,
    lines: &mut Vec<usize>,
) {
    let fields = kinds.fields;
    oracle_walk(node, |node| {
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

#[cfg(test)]
fn collect_oracle(node: Node<'_>, source: &str, kinds: &Kinds, out: &mut Vec<Method>) {
    let fields = kinds.fields;
    let mut scopes: Vec<Scope<'_>> = Vec::new();
    let mut utf16 = Utf16Column::default();
    let mut buffer = Vec::with_capacity(HASH_CHUNK);
    oracle_walk_with_ancestors(node, |node, ancestors| {
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
            metrics_oracle(body, source, kinds, &mut complexity, &mut lines);
            // Expression-bodied arrows have no statement node.
            if kinds.flags(body) & STATEMENT_BLOCK == 0 {
                lines.push(body.start_position().row + 1);
            }
            // Default argument expressions execute as part of the function.
            if let Some(parameters) = Kinds::child(node, fields.parameters) {
                metrics_oracle(parameters, source, kinds, &mut complexity, &mut lines);
            }
            lines.sort_unstable();
            lines.dedup();
            // Hash owned tokens, excluding trivia and descendant functions (which
            // have their own identities). Pruning avoids quadratic hashing of
            // deeply nested functions; line shifts do not change identity.
            let mut hash = Sha256::new();
            buffer.clear();
            oracle_walk(node, |token| {
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

// The single-pass `collect` must match the reference `collect_oracle` in every
// field and in order, on valid and malformed trees alike.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod oracle_tests {
    use super::{Kinds, Method, collect, collect_oracle};
    use std::path::{Path, PathBuf};
    use tree_sitter::{Language, Parser};

    fn languages() -> [(Language, Kinds); 2] {
        [
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            tree_sitter_typescript::LANGUAGE_TSX,
        ]
        .map(|language| {
            let language: Language = language.into();
            let kinds = Kinds::new(&language);
            (language, kinds)
        })
    }

    /// Both collectors on one source; returns the number of methods compared.
    fn assert_same(label: &str, tsx: bool, source: &str) -> usize {
        let (language, kinds) = &languages()[usize::from(tsx)];
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut single = Vec::new();
        collect(tree.root_node(), source, kinds, &mut single);
        let mut oracle = Vec::new();
        collect_oracle(tree.root_node(), source, kinds, &mut oracle);
        assert_eq!(single.len(), oracle.len(), "{label}: method count");
        for (new, old) in single.iter().zip(&oracle) {
            assert_same_method(label, new, old);
        }
        single.len()
    }

    fn assert_same_method(label: &str, new: &Method, old: &Method) {
        let at = format!("{label}: {} ({}:{})", old.name, old.line, old.column);
        assert_eq!(new.name, old.name, "{at}: name");
        assert_eq!(new.identity, old.identity, "{at}: identity");
        assert_eq!(new.line, old.line, "{at}: line");
        assert_eq!(new.column, old.column, "{at}: column");
        assert_eq!(new.column_utf16, old.column_utf16, "{at}: column_utf16");
        assert_eq!(new.end_line, old.end_line, "{at}: end_line");
        assert_eq!(new.complexity, old.complexity, "{at}: complexity");
        assert_eq!(new.lines, old.lines, "{at}: lines");
    }

    fn typescript_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                typescript_files(&path, files);
            } else if kind.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| matches!(extension, "ts" | "tsx" | "mts" | "cts"))
            {
                files.push(path);
            }
        }
    }

    /// Compares every TypeScript file under `dir`; returns (files, methods).
    fn assert_directory_same(dir: &Path) -> (usize, usize) {
        let mut files = Vec::new();
        typescript_files(dir, &mut files);
        files.sort();
        let mut methods = 0;
        let mut compared = 0;
        for path in &files {
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            let tsx = path.extension().is_some_and(|extension| extension == "tsx");
            methods += assert_same(&path.display().to_string(), tsx, &source);
            compared += 1;
        }
        (compared, methods)
    }

    #[test]
    fn single_pass_matches_oracle_on_every_repository_test_file() {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let (files, methods) = assert_directory_same(&tests);
        assert!(files > 0 && methods > 0, "no fixtures compared");
    }

    #[test]
    fn single_pass_matches_oracle_on_tricky_snippets() {
        let snippets: &[(&str, bool, &str)] = &[
            (
                "expression arrows",
                false,
                "const a = (x: number) => x + 1;\nconst b = () => x ? y : z;\nconst c = async () => await f(a && b);\n",
            ),
            (
                "curried arrows",
                false,
                "const f = (a: number) => (b: number) => (c: number) => a + b + c;\nconst g = a => b => { return a || b; };\n",
            ),
            (
                "bodiless overloads with defaults",
                false,
                "function f(a: number): void;\nfunction f(a = () => 1, b = (x) => x ?? 2): void;\nfunction f(a?: any, b?: any) { if (a) return b; }\nclass C { m(a = 1): void; m(a = (z) => z || 1) { return a; } abstract n(x = 2): void; }\ndeclare function d(cb: (x: number) => void): void;\n",
            ),
            (
                "classes inside functions",
                false,
                "function outer() {\n  class Inner { run(x = 1) { return x ? 1 : 2; } static s = () => 1; get v() { return 1; } }\n  const o = class { m() { if (a) {} } };\n  return new Inner();\n}\nclass Top { method() { class Nested { deep() { return () => 1; } } return Nested; } }\n",
            ),
            (
                "comments inside functions",
                false,
                "function f(/* p */ a: number /* q */) {\n  // line\n  /* block */ return a; // tail\n}\nconst g = () => /* c */ 1; // x\n",
            ),
            (
                "default params containing arrows",
                false,
                "function f(cb = () => { if (a) b(); }, n = (x: number) => x > 0 ? 1 : 2) { return cb(n); }\nconst g = ({ a = () => 1, b = 2 } = {}, [c = x || y] = []) => c;\n",
            ),
            (
                "destructuring defaults",
                false,
                "function f({ a = 1, b: { c = 2 } = {} } = {}, [d = 3, e = f2()] = [], ...rest: number[]) { const { x = 1, y = (() => 2)() } = rest; return a; }\n",
            ),
            (
                "getters and setters",
                false,
                "class A {\n  get x(): number { return this._x ?? 0; }\n  set x(v: number) { if (v) this._x = v; }\n  static get y() { return 1; }\n}\nconst o = { get a() { return 1; }, set a(v) { this.b = v; }, m() { return 2; }, k: function () { return 3; }, l: () => 4 };\n",
            ),
            (
                "nested anonymous functions",
                false,
                "export default function () { return [1, 2].map(function (x) { return (() => { return x ? () => 1 : () => 2; })(); }); }\nx.y = function () { return function () { return () => 1; }; };\n(function () { a ||= b; c &&= d; e ??= f; g += 1; })();\n",
            ),
            (
                "type-only and annotations",
                false,
                "function f<T extends { a: () => void }>(x: T, cb: (v: number) => string = (v) => String(v)): T extends string ? 1 : 2 { type U = A extends B ? C : D; let z: (q: number) => void = (q) => {}; return x as any; }\n",
            ),
            (
                "generators and async",
                false,
                "function* g() { yield 1; for (const x of y) { if (x) continue; } }\nconst h = async function* () { while (true) { do { break; } while (a); } };\nswitch (a) { case 1: break; default: }\ntry { f(); } catch (e) { throw e; }\n",
            ),
            (
                "unicode columns",
                false,
                "const s = '😀'; const f = () => 1; const t = '日本語', g = function () { return '😀'; };\n",
            ),
            (
                "tsx components",
                true,
                "export const View = ({ a = () => 1 }: P) => <div onClick={() => a() && b}>{x ? <b>{(() => 1)()}</b> : null}</div>;\nfunction C() { return <>{list.map((i) => <li key={i}>{i}</li>)}</>; }\nclass D extends React.Component { render() { return <p onClick={() => { if (a) b(); }}>x</p>; } }\n",
            ),
            (
                "syntax error",
                false,
                "function f( { return 1; }\nconst g = () => { if (a) { \nclass { m() {} }\n",
            ),
            (
                "missing nodes",
                false,
                "function f() { const x = ; return (1 + ; }\nconst g = (a, b => a;\nfunction h() { foo(1, 2\n",
            ),
            (
                "missing nodes tsx",
                true,
                "const A = () => <div><span>{x ? : 1}</div>;\nfunction B() { return <p>{() => }</p>; }\n",
            ),
            ("empty", false, ""),
            (
                "no functions",
                false,
                "const x = 1; type T = { a: () => void }; interface I { m(): void; n(x = 1): void }\n",
            ),
        ];
        let mut methods = 0;
        for (label, tsx, source) in snippets {
            methods += assert_same(label, *tsx, source);
        }
        assert!(methods > 30, "snippets produced {methods} methods");
    }

    // Set TS_CRAP_ORACLE_CORPUS to a directory of TypeScript to compare on a
    // large real corpus: cargo test --release --lib -- --ignored
    #[test]
    #[ignore = "needs TS_CRAP_ORACLE_CORPUS to name a directory of TypeScript files"]
    fn single_pass_matches_oracle_on_corpus() {
        let dir = std::env::var_os("TS_CRAP_ORACLE_CORPUS")
            .expect("TS_CRAP_ORACLE_CORPUS must name a directory");
        let (files, methods) = assert_directory_same(Path::new(&dir));
        assert!(files > 0, "no TypeScript files under {dir:?}");
        eprintln!("compared {methods} methods in {files} files");
    }
}
