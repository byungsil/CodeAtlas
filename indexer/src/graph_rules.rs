pub const CPP_CALL_RELATIONS: &str = include_str!("../graph/cpp_call_relations.tsg");

#[cfg(test)]
mod tests {
    use super::CPP_CALL_RELATIONS;

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
        assert!(CPP_CALL_RELATIONS.contains("relation_kind = \"type_usage\""));
        assert!(CPP_CALL_RELATIONS.contains("relation_kind = \"inheritance\""));
        assert!(CPP_CALL_RELATIONS.contains("extraction_source = \"tree_sitter_graph\""));
        assert!(CPP_CALL_RELATIONS.contains("file_path = filepath"));
        assert!(CPP_CALL_RELATIONS.contains("line = (plus (start-row @call) 1)"));
    }
}
