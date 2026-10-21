use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use tree_sitter::{Node, Parser};

#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub complexity: u32,
    pub lines: BTreeSet<usize>,
}

fn is_function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
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

fn qualified_name(node: Node<'_>, source: &str) -> String {
    let mut names = vec![local_name(node, source)];
    let mut scope = node.parent();
    while let Some(parent) = scope {
        if is_function(parent) {
            names.push(local_name(parent, source));
        } else if matches!(
            parent.kind(),
            "class_declaration" | "abstract_class_declaration" | "class"
        ) && let Some(class_name) = parent.child_by_field_name("name")
        {
            names.push(text(class_name, source).into());
            break;
        }
        scope = parent.parent();
    }
    names.reverse();
    names.join("::")
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
        if is_function(node) {
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
    walk(node, |node| {
        if is_function(node)
            && let Some(body) = node.child_by_field_name("body")
        {
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
            out.push(Method {
                name: qualified_name(node, source),
                line: node.start_position().row + 1,
                column: node.start_position().column + 1,
                end_line: node.end_position().row + 1,
                complexity,
                lines,
            });
        }
        true
    });
}

pub fn analyze(path: &str, source: &str) -> Result<Vec<Method>> {
    let mut parser = Parser::new();
    let language = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&language.into())
        .context("cannot initialize TypeScript parser")?;
    let tree = parser
        .parse(source, None)
        .context("TypeScript parser failed")?;
    if tree.root_node().has_error() {
        bail!("invalid TypeScript syntax in {path}");
    }
    let mut out = Vec::new();
    collect(tree.root_node(), source, &mut out);
    Ok(out)
}
