use std::collections::HashSet;

use tree_sitter::{Node, Parser, Tree};
use crate::models::{
    ParseResult, RawCallKind, RawCallSite, RawQualifierKind, RawReceiverKind, Symbol,
};

struct Ctx {
    file_path: String,
    source: Vec<u8>,
    symbols: Vec<Symbol>,
    raw_calls: Vec<RawCallSite>,
    ns_stack: Vec<String>,
    namespace_ids: HashSet<String>,
}

impl Ctx {
    fn qualify(&self, name: &str) -> String {
        let mut parts: Vec<&str> = self.ns_stack.iter().map(|s| s.as_str()).collect();
        parts.push(name);
        parts.join("::")
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(&self.source).unwrap_or("").to_string()
    }
}

pub fn parse_cpp_file(file_path: &str, source: &str) -> Result<ParseResult, String> {
    let mut parser = Parser::new();
    let lang = tree_sitter_cpp::LANGUAGE;
    parser
        .set_language(&lang.into())
        .map_err(|e| format!("Failed to set language: {}", e))?;

    let tree: Tree = parser
        .parse(source, None)
        .ok_or_else(|| "Failed to parse".to_string())?;

    let mut ctx = Ctx {
        file_path: file_path.to_string(),
        source: source.as_bytes().to_vec(),
        symbols: Vec::new(),
        raw_calls: Vec::new(),
        ns_stack: Vec::new(),
        namespace_ids: HashSet::new(),
    };

    visit_node(tree.root_node(), &mut ctx);

    Ok(ParseResult {
        symbols: ctx.symbols,
        raw_calls: ctx.raw_calls,
    })
}

fn visit_node(node: Node, ctx: &mut Ctx) {
    match node.kind() {
        "namespace_definition" => visit_namespace(node, ctx),
        "class_specifier" => visit_class(node, ctx, "class"),
        "struct_specifier" => visit_class(node, ctx, "struct"),
        "enum_specifier" => visit_enum(node, ctx),
        "function_definition" => visit_function_definition(node, ctx),
        "declaration" => visit_declaration(node, ctx),
        "template_declaration" => visit_template_declaration(node, ctx),
        _ => visit_children(node, ctx),
    }
}

fn visit_children(node: Node, ctx: &mut Ctx) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node(child, ctx);
    }
}

fn visit_namespace(node: Node, ctx: &mut Ctx) {
    let name_node = node.child_by_field_name("name");
    let body_node = node.child_by_field_name("body");

    if let (Some(name), Some(body)) = (name_node, body_node) {
        let ns_name = ctx.node_text(name);
        let ns_id = ctx.qualify(&ns_name);
        ctx.namespace_ids.insert(ns_id);
        ctx.ns_stack.push(ns_name);
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            visit_node(child, ctx);
        }
        ctx.ns_stack.pop();
    }
}

fn visit_class(node: Node, ctx: &mut Ctx, kind: &str) {
    let name_node = node.child_by_field_name("name");
    let name = match name_node {
        Some(n) => ctx.node_text(n),
        None => return,
    };

    let class_id = ctx.qualify(&name);
    ctx.symbols.push(Symbol {
        id: class_id.clone(),
        name: name.clone(),
        qualified_name: class_id.clone(),
        symbol_type: kind.to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: None,
        parameter_count: None,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: None,
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: None,
    });

    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };

    ctx.ns_stack.push(name);
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "field_declaration" => visit_field_declaration(child, ctx, &class_id),
            "function_definition" => visit_inline_method(child, ctx, &class_id),
            "declaration" => visit_member_declaration(child, ctx, &class_id),
            "class_specifier" => visit_class(child, ctx, "class"),
            "struct_specifier" => visit_class(child, ctx, "struct"),
            "template_declaration" => visit_template_in_class(child, ctx, &class_id),
            _ => {}
        }
    }
    ctx.ns_stack.pop();
}

fn visit_field_declaration(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let func_decl = match find_child(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let method_name = ctx.node_text(name_node);
    let method_id = ctx.qualify(&method_name);
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
    });
}

fn visit_member_declaration(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let func_decl = match find_descendant(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    if name.contains("::") {
        return;
    }
    let method_id = ctx.qualify(&name);
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
    });
}

fn visit_inline_method(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };
    let name_node = match decl_node.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let method_name = ctx.node_text(name_node);
    let method_id = ctx.qualify(&method_name);
    let sig = build_func_signature(node, ctx);
    let parameter_count = count_parameters(decl_node, ctx);

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id.clone(),
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("inline_definition".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: Some(parent_id.to_string()),
    });

    if let Some(body) = node.child_by_field_name("body") {
        extract_call_sites(body, &method_id, ctx);
    }
}

fn visit_enum(node: Node, ctx: &mut Ctx) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    let enum_id = ctx.qualify(&name);

    ctx.symbols.push(Symbol {
        id: enum_id.clone(),
        name,
        qualified_name: enum_id,
        symbol_type: "enum".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: None,
        parameter_count: None,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: None,
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: None,
    });
}

fn visit_function_definition(node: Node, ctx: &mut Ctx) {
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return,
    };

    let func_decl = if decl_node.kind() == "function_declarator" {
        decl_node
    } else {
        match find_descendant(decl_node, "function_declarator") {
            Some(fd) => fd,
            None => return,
        }
    };

    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };

    let (func_name, func_id, parent_id) = if name_node.kind() == "qualified_identifier" {
        let (class_name, member_name) = parse_qualified_id(name_node, ctx);
        if !class_name.is_empty() {
            let class_id = ctx.qualify(&class_name);
            let full_id = format!("{}::{}", class_id, member_name);
            (member_name, full_id, Some(class_id))
        } else {
            let text = ctx.node_text(name_node);
            let id = ctx.qualify(&text);
            (text, id, None)
        }
    } else if name_node.kind() == "destructor_name" {
        let text = ctx.node_text(name_node);
        let id = ctx.qualify(&text);
        (text, id, None)
    } else {
        let text = ctx.node_text(name_node);
        let id = ctx.qualify(&text);
        (text, id, None)
    };

    let sig = build_func_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);
    let sym_type = if parent_id.is_some() { "method" } else { "function" };

    ctx.symbols.push(Symbol {
        id: func_id.clone(),
        name: func_name,
        qualified_name: func_id.clone(),
        symbol_type: sym_type.to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("definition".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id,
    });

    if let Some(body) = node.child_by_field_name("body") {
        extract_call_sites(body, &func_id, ctx);
    }
}

fn visit_declaration(node: Node, ctx: &mut Ctx) {
    let func_decl = match find_descendant(node, "function_declarator") {
        Some(fd) => fd,
        None => return,
    };
    let name_node = match func_decl.child_by_field_name("declarator") {
        Some(n) => n,
        None => return,
    };
    let name = ctx.node_text(name_node);
    if name.contains("::") {
        return;
    }
    let func_id = ctx.qualify(&name);
    if ctx.symbols.iter().any(|s| s.id == func_id) {
        return;
    }
    let sig = build_decl_signature(node, ctx);
    let parameter_count = count_parameters(func_decl, ctx);

    ctx.symbols.push(Symbol {
        id: func_id.clone(),
        name,
        qualified_name: func_id,
        symbol_type: "function".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
        parameter_count,
        scope_qualified_name: None,
        scope_kind: None,
        symbol_role: Some("declaration".to_string()),
        declaration_file_path: None,
        declaration_line: None,
        declaration_end_line: None,
        definition_file_path: None,
        definition_line: None,
        definition_end_line: None,
        parent_id: None,
    });
}

fn visit_template_declaration(node: Node, ctx: &mut Ctx) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_specifier" => visit_class(child, ctx, "class"),
            "struct_specifier" => visit_class(child, ctx, "struct"),
            "function_definition" => visit_function_definition(child, ctx),
            "declaration" => visit_declaration(child, ctx),
            _ => {}
        }
    }
}

fn visit_template_in_class(node: Node, ctx: &mut Ctx, parent_id: &str) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => visit_inline_method(child, ctx, parent_id),
            "field_declaration" => visit_field_declaration(child, ctx, parent_id),
            "declaration" => visit_member_declaration(child, ctx, parent_id),
            _ => {}
        }
    }
}

fn extract_call_sites(node: Node, caller_id: &str, ctx: &mut Ctx) {
    if node.kind() == "call_expression" {
        if let Some(func_node) = node.child_by_field_name("function") {
            if let Some(raw_call) = parse_call_expr(func_node, caller_id, node, ctx) {
                ctx.raw_calls.push(RawCallSite {
                    file_path: ctx.file_path.clone(),
                    line: node.start_position().row + 1,
                    ..raw_call
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_call_sites(child, caller_id, ctx);
    }
}

fn parse_call_expr(func_node: Node, caller_id: &str, call_node: Node, ctx: &Ctx) -> Option<RawCallSite> {
    let argument_count = call_node
        .child_by_field_name("arguments")
        .map(|args| count_arguments(args, ctx));

    match func_node.kind() {
        "identifier" => Some(RawCallSite {
            caller_id: caller_id.to_string(),
            called_name: ctx.node_text(func_node),
            call_kind: RawCallKind::Unqualified,
            argument_count,
            receiver: None,
            receiver_kind: None,
            qualifier: None,
            qualifier_kind: None,
            file_path: String::new(),
            line: call_node.start_position().row + 1,
        }),
        "field_expression" => {
            let field = func_node.child_by_field_name("field")?;
            let arg = func_node.child_by_field_name("argument");
            let receiver = arg.map(|a| extract_receiver(a, ctx));
            let receiver_kind = arg.map(detect_receiver_kind);
            let separator = field_expr_separator(func_node, ctx);
            let call_kind = match (receiver.as_deref(), separator.as_str()) {
                (Some("this"), "->") => RawCallKind::ThisPointerAccess,
                (_, "->") => RawCallKind::PointerMemberAccess,
                _ => RawCallKind::MemberAccess,
            };
            Some(RawCallSite {
                caller_id: caller_id.to_string(),
                called_name: ctx.node_text(field),
                call_kind,
                argument_count,
                receiver,
                receiver_kind,
                qualifier: None,
                qualifier_kind: None,
                file_path: String::new(),
                line: call_node.start_position().row + 1,
            })
        }
        "qualified_identifier" => {
            let count = func_node.named_child_count();
            let last = func_node.named_child(count.checked_sub(1)?)?;
            let mut parts = Vec::new();
            for i in 0..count.saturating_sub(1) {
                if let Some(child) = func_node.named_child(i) {
                    parts.push(ctx.node_text(child));
                }
            }
            let qualifier = if parts.is_empty() {
                None
            } else {
                Some(parts.join("::"))
            };
            let qualifier_kind = qualifier
                .as_deref()
                .and_then(|value| classify_qualifier_kind(value, ctx));
            Some(RawCallSite {
                caller_id: caller_id.to_string(),
                called_name: ctx.node_text(last),
                call_kind: RawCallKind::Qualified,
                argument_count,
                receiver: None,
                receiver_kind: None,
                qualifier,
                qualifier_kind,
                file_path: String::new(),
                line: call_node.start_position().row + 1,
            })
        }
        _ => None,
    }
}

fn extract_receiver(node: Node, ctx: &Ctx) -> String {
    match node.kind() {
        "identifier" | "this" => ctx.node_text(node),
        "pointer_expression" => {
            if let Some(arg) = node.child_by_field_name("argument") {
                extract_receiver(arg, ctx)
            } else {
                ctx.node_text(node)
            }
        }
        _ => ctx.node_text(node),
    }
}

fn detect_receiver_kind(node: Node) -> RawReceiverKind {
    match node.kind() {
        "identifier" => RawReceiverKind::Identifier,
        "this" => RawReceiverKind::This,
        "pointer_expression" => RawReceiverKind::PointerExpression,
        "field_expression" => RawReceiverKind::FieldExpression,
        "qualified_identifier" => RawReceiverKind::QualifiedIdentifier,
        _ => RawReceiverKind::Other,
    }
}

fn field_expr_separator(node: Node, ctx: &Ctx) -> String {
    let argument = match node.child_by_field_name("argument") {
        Some(arg) => arg,
        None => return ".".to_string(),
    };
    let field = match node.child_by_field_name("field") {
        Some(field) => field,
        None => return ".".to_string(),
    };
    let start = argument.end_byte();
    let end = field.start_byte();
    if start >= end || end > ctx.source.len() {
        return ".".to_string();
    }
    let between = std::str::from_utf8(&ctx.source[start..end]).unwrap_or("");
    if between.contains("->") {
        "->".to_string()
    } else {
        ".".to_string()
    }
}

fn classify_qualifier_kind(qualifier: &str, ctx: &Ctx) -> Option<RawQualifierKind> {
    if is_known_type_qualifier(qualifier, ctx) {
        return Some(RawQualifierKind::Type);
    }

    if is_known_namespace_qualifier(qualifier, ctx) {
        return Some(RawQualifierKind::Namespace);
    }

    None
}

fn is_known_type_qualifier(qualifier: &str, ctx: &Ctx) -> bool {
    let contextual = ctx.qualify(qualifier);
    ctx.symbols.iter().any(|sym| {
        (sym.symbol_type == "class" || sym.symbol_type == "struct")
            && (sym.qualified_name == qualifier
                || sym.qualified_name == contextual
                || sym.name == qualifier)
    })
}

fn is_known_namespace_qualifier(qualifier: &str, ctx: &Ctx) -> bool {
    ctx.namespace_ids.contains(qualifier) || ctx.namespace_ids.contains(&ctx.qualify(qualifier))
}

fn parse_qualified_id(node: Node, ctx: &Ctx) -> (String, String) {
    let count = node.named_child_count();
    if count >= 2 {
        let mut parts = Vec::new();
        for i in 0..count {
            if let Some(child) = node.named_child(i) {
                parts.push(ctx.node_text(child));
            }
        }
        let member = parts.pop().unwrap_or_default();
        let class = parts.join("::");
        (class, member)
    } else {
        (String::new(), ctx.node_text(node))
    }
}

fn build_func_signature(node: Node, ctx: &Ctx) -> String {
    let return_type = build_return_type(node, ctx);
    let decl_node = match node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return String::new(),
    };

    let func_decl = if decl_node.kind() == "function_declarator" {
        decl_node
    } else {
        match find_descendant(decl_node, "function_declarator") {
            Some(fd) => fd,
            None => return format!("{} {}", return_type, ctx.node_text(decl_node)).trim().to_string(),
        }
    };

    let name = func_decl
        .child_by_field_name("declarator")
        .map(|n| ctx.node_text(n))
        .unwrap_or_default();
    let params = func_decl
        .child_by_field_name("parameters")
        .map(|n| ctx.node_text(n))
        .unwrap_or_else(|| "()".to_string());

    let mut qualifiers = Vec::new();
    let mut cursor = func_decl.walk();
    for child in func_decl.children(&mut cursor) {
        if child.kind() == "type_qualifier" {
            qualifiers.push(ctx.node_text(child));
        }
    }

    let qual = if qualifiers.is_empty() {
        String::new()
    } else {
        format!(" {}", qualifiers.join(" "))
    };

    let prefix = if return_type.is_empty() {
        String::new()
    } else {
        format!("{} ", return_type)
    };

    format!("{}{}{}{}", prefix, name, params, qual).trim().to_string()
}

fn build_return_type(node: Node, ctx: &Ctx) -> String {
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_qualifier" | "primitive_type" | "type_identifier"
            | "qualified_identifier" | "sized_type_specifier" | "template_type" => {
                parts.push(ctx.node_text(child));
            }
            _ => {
                if child.kind().contains("declarator") || child.kind() == "compound_statement"
                    || child.kind() == "field_initializer_list"
                {
                    break;
                }
            }
        }
    }

    let decl_node = node.child_by_field_name("declarator");
    if let Some(d) = decl_node {
        match d.kind() {
            "reference_declarator" => {
                if find_descendant(d, "function_declarator").is_some() {
                    parts.push("&".to_string());
                }
            }
            "pointer_declarator" => {
                parts.push("*".to_string());
            }
            _ => {}
        }
    }

    parts.join(" ")
}

fn build_decl_signature(node: Node, ctx: &Ctx) -> String {
    let text = ctx.node_text(node);
    let trimmed = text.trim_end_matches(';').trim_end();
    if let Some(brace) = trimmed.find('{') {
        trimmed[..brace].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn count_parameters(func_decl: Node, ctx: &Ctx) -> Option<usize> {
    let params = func_decl.child_by_field_name("parameters")?;
    Some(count_parameter_like_list(params, ctx))
}

fn count_arguments(args_node: Node, ctx: &Ctx) -> usize {
    count_parameter_like_list(args_node, ctx)
}

fn count_parameter_like_list(list_node: Node, ctx: &Ctx) -> usize {
    let named_count = list_node.named_child_count();
    if named_count == 0 {
        return 0;
    }

    if named_count == 1 {
        if let Some(child) = list_node.named_child(0) {
            let text = ctx.node_text(child);
            if child.kind() == "primitive_type" && text == "void" {
                return 0;
            }
        }
    }

    named_count
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node.children(&mut cursor).find(|c| c.kind() == kind);
    result
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_function() {
        let src = "void foo(int x) { bar(x); }";
        let result = parse_cpp_file("test.cpp", src).unwrap();
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "foo");
        assert_eq!(result.symbols[0].symbol_type, "function");
        assert_eq!(result.symbols[0].parameter_count, Some(1));
        assert!(result.raw_calls.iter().any(|c| {
            c.called_name == "bar"
                && matches!(c.call_kind, RawCallKind::Unqualified)
                && c.argument_count == Some(1)
        }));
    }

    #[test]
    fn parses_namespace_and_class() {
        let src = r#"
namespace Game {
class Player {
public:
    void Update(float dt);
};
}
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "Player").unwrap();
        assert_eq!(class.id, "Game::Player");
        let method = result.symbols.iter().find(|s| s.name == "Update").unwrap();
        assert_eq!(method.id, "Game::Player::Update");
        assert_eq!(method.parent_id.as_deref(), Some("Game::Player"));
    }

    #[test]
    fn parses_method_definition() {
        let src = r#"
namespace Game {
void Player::Update(float dt) {
    Render();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let method = result.symbols.iter().find(|s| s.name == "Update").unwrap();
        assert_eq!(method.id, "Game::Player::Update");
        assert_eq!(method.symbol_type, "method");
        assert_eq!(method.parameter_count, Some(1));
        assert!(result.raw_calls.iter().any(|c| {
            c.called_name == "Render" && matches!(c.call_kind, RawCallKind::Unqualified)
        }));
    }

    #[test]
    fn parses_enum_class() {
        let src = "enum class AIState { Idle, Patrol, Chase };";
        let result = parse_cpp_file("test.h", src).unwrap();
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "AIState");
        assert_eq!(result.symbols[0].symbol_type, "enum");
    }

    #[test]
    fn parses_template_class() {
        let src = r#"
template <typename T>
class Container {
public:
    void Add(const T& item);
    T Get(int index) const;
};
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "Container").unwrap();
        assert_eq!(class.symbol_type, "class");
        assert!(result.symbols.iter().any(|s| s.name == "Add"));
        assert!(result.symbols.iter().any(|s| s.name == "Get"));
    }

    #[test]
    fn parses_template_function() {
        let src = r#"
template <typename T>
T Max(T a, T b) { return a > b ? a : b; }
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        assert!(result.symbols.iter().any(|s| {
            s.name == "Max"
                && s.symbol_type == "function"
                && s.symbol_role.as_deref() == Some("definition")
        }));
    }

    #[test]
    fn tolerates_macro_heavy_code() {
        let src = r#"
#define DECLARE_CLASS(name) class name##Impl {};
#ifdef SOME_FLAG
void conditionalFunc() {}
#endif
#if defined(OTHER_FLAG)
void otherFunc() {}
#else
void fallbackFunc() {}
#endif
void alwaysPresent() {}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        assert!(result.symbols.iter().any(|s| s.name == "alwaysPresent"));
    }

    #[test]
    fn per_file_failure_isolation() {
        let malformed = "class { void broken( {}";
        let result = parse_cpp_file("bad.cpp", malformed);
        assert!(result.is_ok());
    }

    #[test]
    fn parses_deep_nested_namespace() {
        let src = r#"
namespace EA {
namespace Ant {
namespace Controllers {
class ControllerAsset {
public:
    void Load();
};
}}}
"#;
        let result = parse_cpp_file("test.h", src).unwrap();
        let class = result.symbols.iter().find(|s| s.name == "ControllerAsset").unwrap();
        assert_eq!(class.id, "EA::Ant::Controllers::ControllerAsset");
        let method = result.symbols.iter().find(|s| s.name == "Load").unwrap();
        assert_eq!(method.parent_id.as_deref(), Some("EA::Ant::Controllers::ControllerAsset"));
    }

    #[test]
    fn parses_member_call_shapes() {
        let src = r#"
namespace Game {
class Worker {
public:
    void Update();
    void Tick(Worker* other) {
        this->Update();
        other->Update();
    }
};
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let this_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .unwrap();
        assert_eq!(this_call.called_name, "Update");
        assert_eq!(this_call.receiver.as_deref(), Some("this"));
        assert_eq!(this_call.receiver_kind, Some(RawReceiverKind::This));

        let pointer_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::PointerMemberAccess))
            .unwrap();
        assert_eq!(pointer_call.called_name, "Update");
        assert_eq!(pointer_call.receiver.as_deref(), Some("other"));
        assert_eq!(pointer_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }

    #[test]
    fn parses_dot_member_call_shape() {
        let src = r#"
namespace Game {
class Worker {
public:
    void Update();
};

void Tick() {
    Worker local;
    local.Update();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let member_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::MemberAccess))
            .unwrap();
        assert_eq!(member_call.called_name, "Update");
        assert_eq!(member_call.argument_count, Some(0));
        assert_eq!(member_call.receiver.as_deref(), Some("local"));
        assert_eq!(member_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }

    #[test]
    fn parses_namespace_qualified_call_shape() {
        let src = r#"
namespace Gameplay {
void Update();
}

namespace AI {
class Controller {
public:
    void Tick() {
        Gameplay::Update();
    }
};
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let qualified_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .unwrap();
        assert_eq!(qualified_call.called_name, "Update");
        assert_eq!(qualified_call.argument_count, Some(0));
        assert_eq!(qualified_call.qualifier.as_deref(), Some("Gameplay"));
        assert_eq!(qualified_call.qualifier_kind, Some(RawQualifierKind::Namespace));
    }

    #[test]
    fn parses_type_qualified_call_shape() {
        let src = r#"
namespace Game {
class Worker {
public:
    static void Update();
};

void Tick() {
    Worker::Update();
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let qualified_call = result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .unwrap();
        assert_eq!(qualified_call.called_name, "Update");
        assert_eq!(qualified_call.argument_count, Some(0));
        assert_eq!(qualified_call.qualifier.as_deref(), Some("Worker"));
        assert_eq!(qualified_call.qualifier_kind, Some(RawQualifierKind::Type));
    }

    #[test]
    fn parses_parameter_and_argument_counts() {
        let src = r#"
namespace Math {
void Blend(int a);
void Compose(float a, float b);

void Tick() {
    Blend(1);
    Compose(1.0f, 2.0f);
}
}
"#;
        let result = parse_cpp_file("test.cpp", src).unwrap();
        let one_arg = result
            .symbols
            .iter()
            .find(|s| s.signature.as_deref() == Some("void Blend(int a)"))
            .unwrap();
        let two_arg = result
            .symbols
            .iter()
            .find(|s| s.signature.as_deref() == Some("void Compose(float a, float b)"))
            .unwrap();
        assert_eq!(one_arg.parameter_count, Some(1));
        assert_eq!(two_arg.parameter_count, Some(2));

        assert!(result
            .raw_calls
            .iter()
            .any(|c| c.called_name == "Blend" && c.argument_count == Some(1)));
        assert!(result
            .raw_calls
            .iter()
            .any(|c| c.called_name == "Compose" && c.argument_count == Some(2)));
    }

    #[test]
    fn tags_free_function_declaration_and_definition_roles() {
        let decl_src = "void Tick();";
        let decl_result = parse_cpp_file("test.h", decl_src).unwrap();
        let decl = decl_result.symbols.iter().find(|s| s.name == "Tick").unwrap();
        assert_eq!(decl.symbol_role.as_deref(), Some("declaration"));

        let def_src = "void Tick() {}";
        let def_result = parse_cpp_file("test.cpp", def_src).unwrap();
        let def = def_result.symbols.iter().find(|s| s.name == "Tick").unwrap();
        assert_eq!(def.symbol_role.as_deref(), Some("definition"));
    }

    #[test]
    fn tags_method_declaration_definition_and_inline_definition_roles() {
        let decl_src = r#"
namespace Game {
class Worker {
public:
    void Update();
};
}
"#;
        let decl_result = parse_cpp_file("worker.h", decl_src).unwrap();
        let decl = decl_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(decl.symbol_role.as_deref(), Some("declaration"));

        let def_src = r#"
namespace Game {
void Worker::Update() {}
}
"#;
        let def_result = parse_cpp_file("worker.cpp", def_src).unwrap();
        let def = def_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(def.symbol_role.as_deref(), Some("definition"));

        let inline_src = r#"
namespace Game {
class Worker {
public:
    void Update() {}
};
}
"#;
        let inline_result = parse_cpp_file("worker_inline.h", inline_src).unwrap();
        let inline = inline_result
            .symbols
            .iter()
            .find(|s| s.id == "Game::Worker::Update")
            .unwrap();
        assert_eq!(inline.symbol_role.as_deref(), Some("inline_definition"));
    }

    #[test]
    fn ambiguity_namespace_fixture_parses_qualified_calls() {
        let src = include_str!("../../samples/ambiguity/src/namespace_dupes.cpp");
        let result = parse_cpp_file("samples/ambiguity/src/namespace_dupes.cpp", src).unwrap();

        let controller = result
            .symbols
            .iter()
            .find(|s| s.id == "AI::Controller::Update")
            .unwrap();
        assert_eq!(controller.symbol_role.as_deref(), Some("definition"));

        let qualified_calls: Vec<_> = result
            .raw_calls
            .iter()
            .filter(|c| matches!(c.call_kind, RawCallKind::Qualified))
            .collect();
        assert_eq!(qualified_calls.len(), 2);
        assert!(qualified_calls.iter().any(|c| {
            c.called_name == "Update"
                && c.qualifier.as_deref() == Some("Gameplay")
                && c.qualifier_kind == Some(RawQualifierKind::Namespace)
        }));
        assert!(qualified_calls.iter().any(|c| {
            c.called_name == "Update"
                && c.qualifier.as_deref() == Some("UI")
                && c.qualifier_kind == Some(RawQualifierKind::Namespace)
        }));
    }

    #[test]
    fn ambiguity_overloads_fixture_parses_arity_metadata() {
        let header = include_str!("../../samples/ambiguity/src/overloads.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/overloads.h", header).unwrap();

        let decls: Vec<_> = header_result
            .symbols
            .iter()
            .filter(|s| s.name == "Blend")
            .collect();
        assert_eq!(decls.len(), 2);
        assert!(decls.iter().any(|s| {
            s.id == "Math::Blend"
                && s.parameter_count == Some(1)
                && s.symbol_role.as_deref() == Some("declaration")
        }));
        assert!(decls.iter().any(|s| {
            s.id == "Renderer::Blend"
                && s.parameter_count == Some(2)
                && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/overloads.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/overloads.cpp", source).unwrap();
        let blend_call = source_result
            .raw_calls
            .iter()
            .find(|c| c.caller_id == "Renderer::Blend" && c.called_name == "Blend")
            .unwrap();
        assert_eq!(blend_call.argument_count, Some(1));
        assert!(matches!(blend_call.call_kind, RawCallKind::Qualified));
        assert_eq!(blend_call.qualifier.as_deref(), Some("Math"));
        assert_eq!(blend_call.qualifier_kind, Some(RawQualifierKind::Namespace));
    }

    #[test]
    fn ambiguity_sibling_methods_fixture_parses_this_calls() {
        let header = include_str!("../../samples/ambiguity/src/sibling_methods.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/sibling_methods.h", header).unwrap();
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Player::Process" && s.symbol_role.as_deref() == Some("declaration")
        }));
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Enemy::Run" && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/sibling_methods.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/sibling_methods.cpp", source).unwrap();
        let this_calls: Vec<_> = source_result
            .raw_calls
            .iter()
            .filter(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .collect();
        assert_eq!(this_calls.len(), 2);
        assert!(this_calls.iter().all(|c| c.called_name == "Run"));
        assert!(this_calls.iter().all(|c| c.receiver.as_deref() == Some("this")));
        assert!(this_calls.iter().all(|c| c.receiver_kind == Some(RawReceiverKind::This)));
    }

    #[test]
    fn ambiguity_split_update_fixture_parses_split_roles_and_pointer_calls() {
        let header = include_str!("../../samples/ambiguity/src/split_update.h");
        let header_result = parse_cpp_file("samples/ambiguity/src/split_update.h", header).unwrap();
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Update"
                && s.symbol_role.as_deref() == Some("declaration")
                && s.parent_id.as_deref() == Some("Game::Worker")
        }));
        assert!(header_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Tick"
                && s.parameter_count == Some(1)
                && s.symbol_role.as_deref() == Some("declaration")
        }));

        let source = include_str!("../../samples/ambiguity/src/split_update.cpp");
        let source_result = parse_cpp_file("samples/ambiguity/src/split_update.cpp", source).unwrap();
        assert!(source_result.symbols.iter().any(|s| {
            s.id == "Game::Worker::Update" && s.symbol_role.as_deref() == Some("definition")
        }));

        let this_call = source_result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::ThisPointerAccess))
            .unwrap();
        assert_eq!(this_call.called_name, "Update");
        assert_eq!(this_call.receiver.as_deref(), Some("this"));

        let pointer_call = source_result
            .raw_calls
            .iter()
            .find(|c| matches!(c.call_kind, RawCallKind::PointerMemberAccess))
            .unwrap();
        assert_eq!(pointer_call.called_name, "Update");
        assert_eq!(pointer_call.receiver.as_deref(), Some("other"));
        assert_eq!(pointer_call.receiver_kind, Some(RawReceiverKind::Identifier));
    }
}
