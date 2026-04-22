pub const CPP_CALL_RELATIONS: &str = include_str!("../graph/cpp_call_relations.tsg");
pub const TYPESCRIPT_CALL_RELATIONS: &str = include_str!("../graph/typescript_call_relations.tsg");
pub const PYTHON_CALL_RELATIONS: &str = include_str!("../graph/python_call_relations.tsg");
pub const RUST_CALL_RELATIONS: &str = include_str!("../graph/rust_call_relations.tsg");
pub const LUA_CALL_RELATIONS: &str = include_str!("../graph/lua_call_relations.tsg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpp_call_relations_rule_file_covers_initial_call_shapes() {
        assert!(CPP_CALL_RELATIONS.contains("call_kind = \"unqualified\""));
        assert!(CPP_CALL_RELATIONS.contains("call_kind = \"qualified\""));
        assert!(CPP_CALL_RELATIONS.contains("call_kind = \"member_access\""));
        assert!(CPP_CALL_RELATIONS.contains("call_kind = \"pointer_member_access\""));
        assert!(CPP_CALL_RELATIONS.contains("call_kind = \"this_pointer_access\""));
    }

    #[test]
    fn cpp_call_relations_rule_file_keeps_output_event_shaped() {
        assert!(CPP_CALL_RELATIONS.contains("relation_kind = \"call\""));
        assert!(CPP_CALL_RELATIONS.contains("line = (plus (start-row @call) 1)"));
        assert!(CPP_CALL_RELATIONS.contains("target_name = (source-text @callee)"));
    }

    #[test]
    fn typescript_call_relations_rule_file_covers_call_shapes() {
        assert!(TYPESCRIPT_CALL_RELATIONS.contains("call_kind = \"unqualified\""));
        assert!(TYPESCRIPT_CALL_RELATIONS.contains("call_kind = \"member_access\""));
        assert!(TYPESCRIPT_CALL_RELATIONS.contains("call_kind = \"this_pointer_access\""));
        assert!(TYPESCRIPT_CALL_RELATIONS.contains("relation_kind = \"call\""));
        assert!(TYPESCRIPT_CALL_RELATIONS.contains("target_name = (source-text @callee)"));
    }

    #[test]
    fn python_call_relations_rule_file_covers_call_shapes() {
        assert!(PYTHON_CALL_RELATIONS.contains("call_kind = \"unqualified\""));
        assert!(PYTHON_CALL_RELATIONS.contains("call_kind = \"member_access\""));
        assert!(PYTHON_CALL_RELATIONS.contains("relation_kind = \"call\""));
        assert!(PYTHON_CALL_RELATIONS.contains("target_name = (source-text @callee)"));
    }

    #[test]
    fn rust_call_relations_rule_file_covers_call_shapes() {
        assert!(RUST_CALL_RELATIONS.contains("call_kind = \"unqualified\""));
        assert!(RUST_CALL_RELATIONS.contains("call_kind = \"qualified\""));
        assert!(RUST_CALL_RELATIONS.contains("call_kind = \"member_access\""));
        assert!(RUST_CALL_RELATIONS.contains("call_kind = \"this_pointer_access\""));
        assert!(RUST_CALL_RELATIONS.contains("relation_kind = \"call\""));
    }

    #[test]
    fn lua_call_relations_rule_file_covers_call_shapes() {
        assert!(LUA_CALL_RELATIONS.contains("call_kind = \"unqualified\""));
        assert!(LUA_CALL_RELATIONS.contains("call_kind = \"member_access\""));
        assert!(LUA_CALL_RELATIONS.contains("call_kind = \"this_pointer_access\""));
        assert!(LUA_CALL_RELATIONS.contains("relation_kind = \"call\""));
    }
}
