use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use tree_sitter::{Node, Parser};

#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub identity: String,
    pub line: usize,
    pub column: usize,
    pub column_utf16: usize,
    pub end_line: usize,
    pub complexity: u32,
    pub lines: BTreeSet<usize>,
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

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn local_name(node: Node<'_>, source: &str) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return text(name, source).into();
    }
    if let Some(parent) = node.parent() {
        match parent.kind() {
            "variable_declarator" | "public_field_definition" | "pair" => {
                if let Some(name) = parent
                    .child_by_field_name("name")
                    .or_else(|| parent.child_by_field_name("key"))
                {
                    return text(name, source).into();
                }
            }
            "assignment_expression" => {
                if let Some(left) = parent.child_by_field_name("left") {
                    return text(left, source).into();
                }
            }
            _ => {}
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

// Each decision adds one independent execution path. Descendant functions are
// scored separately, never charged to their enclosing function.
fn metrics(node: Node<'_>, source: &str, complexity: &mut u32, lines: &mut BTreeSet<usize>) {
    walk(node, |node| {
        if is_function(node.kind()) {
            return false;
        }
        // Type-only branches such as conditional types do not affect runtime flow.
        if matches!(
            node.kind(),
            "type_annotation" | "type_parameters" | "return_type"
        ) {
            return false;
        }
        if matches!(
            node.kind(),
            "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "catch_clause"
                | "switch_case"
                | "ternary_expression"
        ) {
            *complexity += 1;
        }
        if matches!(
            node.kind(),
            "binary_expression" | "augmented_assignment_expression"
        ) && let Some(operator) = node.child_by_field_name("operator")
            && matches!(
                text(operator, source),
                "&&" | "||" | "??" | "&&=" | "||=" | "??="
            )
        {
            *complexity += 1;
        }
        if matches!(
            node.kind(),
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
            lines.insert(node.start_position().row + 1);
        }
        let initializer = match node.kind() {
            "required_parameter" | "optional_parameter" => node.child_by_field_name("value"),
            "assignment_pattern" | "object_assignment_pattern" => node.child_by_field_name("right"),
            _ => None,
        };
        if let Some(value) = initializer {
            lines.insert(value.start_position().row + 1);
        }
        true
    });
}

fn collect(node: Node<'_>, source: &str, out: &mut Vec<Method>) {
    let mut scopes: Vec<Scope<'_>> = Vec::new();
    let mut utf16 = Utf16Column::default();
    walk(node, |node| {
        let kind = node.kind();
        let class_name = if matches!(
            kind,
            "class_declaration" | "abstract_class_declaration" | "class"
        ) {
            node.child_by_field_name("name")
        } else {
            None
        };
        if !is_function(kind) && class_name.is_none() {
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
        } else if let Some(body) = node.child_by_field_name("body") {
            let local = local_name(node, source);
            let name = match scopes.last().map(|scope| &scope.name) {
                Some(ScopeName::Class(class)) => format!("{class}::{local}"),
                Some(ScopeName::Function(index)) => format!("{}::{local}", out[*index].name),
                None => local,
            };
            let mut complexity = 1;
            let mut lines = BTreeSet::new();
            metrics(body, source, &mut complexity, &mut lines);
            // Expression-bodied arrows have no statement node.
            if body.kind() != "statement_block" {
                lines.insert(body.start_position().row + 1);
            }
            // Default argument expressions execute as part of the function.
            if let Some(parameters) = node.child_by_field_name("parameters") {
                metrics(parameters, source, &mut complexity, &mut lines);
            }
            // Hash owned tokens, excluding trivia and descendant functions (which
            // have their own identities). Pruning avoids quadratic hashing of
            // deeply nested functions; line shifts do not change identity.
            let mut hash = Sha256::new();
            walk(node, |token| {
                if token.kind() == "comment" || (token != node && is_function(token.kind())) {
                    return false;
                }
                if token.child_count() == 0 {
                    hash.update(token.kind().as_bytes());
                    hash.update([0]);
                    hash.update(text(token, source).as_bytes());
                    hash.update([0]);
                }
                true
            });
            out.push(Method {
                identity: hex::encode(hash.finalize()),
                name,
                line: node.start_position().row + 1,
                column: node.start_position().column + 1,
                column_utf16: utf16.at(node, source),
                end_line: node.end_position().row + 1,
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
    typescript: Option<Parser>,
    tsx: Option<Parser>,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyzer")
            .field("typescript_initialized", &self.typescript.is_some())
            .field("tsx_initialized", &self.tsx.is_some())
            .finish()
    }
}

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
            let language = if tsx {
                tree_sitter_typescript::LANGUAGE_TSX
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT
            };
            parser
                .set_language(&language.into())
                .context("cannot initialize TypeScript parser")?;
            *slot = Some(parser);
        }
        let tree = slot
            .as_mut()
            .context("TypeScript parser unavailable")?
            .parse(source, None)
            .context("TypeScript parser failed")?;
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
            let snippet: String = source
                .lines()
                .nth(position.row)
                .unwrap_or("")
                .chars()
                .take(160)
                .collect();
            bail!(
                "invalid TypeScript syntax in {path}:{}:{} (UTF-8 byte column): {snippet}",
                position.row + 1,
                position.column + 1
            );
        }
        let mut out = Vec::new();
        collect(tree.root_node(), source, &mut out);
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
