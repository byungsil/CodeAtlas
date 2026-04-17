use tree_sitter::{Node, Parser, Tree};
use crate::models::{ParseResult, RawCallSite, Symbol};

struct Ctx {
    file_path: String,
    source: Vec<u8>,
    symbols: Vec<Symbol>,
    raw_calls: Vec<RawCallSite>,
    ns_stack: Vec<String>,
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

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
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

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name,
        qualified_name: method_id,
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
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

    ctx.symbols.push(Symbol {
        id: method_id.clone(),
        name: method_name,
        qualified_name: method_id.clone(),
        symbol_type: "method".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
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

    ctx.symbols.push(Symbol {
        id: func_id.clone(),
        name,
        qualified_name: func_id,
        symbol_type: "function".to_string(),
        file_path: ctx.file_path.clone(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        signature: Some(sig),
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
            if let Some((name, receiver)) = parse_call_expr(func_node, ctx) {
                ctx.raw_calls.push(RawCallSite {
                    caller_id: caller_id.to_string(),
                    called_name: name,
                    receiver,
                    file_path: ctx.file_path.clone(),
                    line: node.start_position().row + 1,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_call_sites(child, caller_id, ctx);
    }
}

fn parse_call_expr(func_node: Node, ctx: &Ctx) -> Option<(String, Option<String>)> {
    match func_node.kind() {
        "identifier" => Some((ctx.node_text(func_node), None)),
        "field_expression" => {
            let field = func_node.child_by_field_name("field")?;
            let arg = func_node.child_by_field_name("argument");
            let receiver = arg.map(|a| extract_receiver(a, ctx));
            Some((ctx.node_text(field), receiver))
        }
        "qualified_identifier" => {
            let last = func_node.named_child(func_node.named_child_count().checked_sub(1)?)?;
            Some((ctx.node_text(last), None))
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
        assert!(result.raw_calls.iter().any(|c| c.called_name == "bar"));
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
        assert!(result.raw_calls.iter().any(|c| c.called_name == "Render"));
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
        assert!(result.symbols.iter().any(|s| s.name == "Max" && s.symbol_type == "function"));
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
}
