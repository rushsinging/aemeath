use super::*;

#[test]
fn test_same_provider_id_reuses_same_tool_call_id() {
    let registry = ToolIdentityRegistry::new();
    let id1 = registry.runtime_id_for_provider("provider-a");
    let id2 = registry.runtime_id_for_provider("provider-a");
    assert_eq!(id1, id2);
}

#[test]
fn test_different_provider_ids_generate_different_tool_call_ids() {
    let registry = ToolIdentityRegistry::new();
    let id1 = registry.runtime_id_for_provider("provider-a");
    let id2 = registry.runtime_id_for_provider("provider-b");
    assert_ne!(id1, id2);
}

#[test]
fn test_stream_index_binds_to_provider_id_later() {
    let registry = ToolIdentityRegistry::new();
    let id_by_provider = registry.runtime_id_for_stream(0, Some("provider-a"));
    let id_by_index = registry.runtime_id_for_stream(0, None);
    assert_eq!(id_by_provider, id_by_index);
}

#[test]
fn test_all_ids_are_uuidv7() {
    let registry = ToolIdentityRegistry::new();
    let id1 = registry.runtime_id_for_stream(0, None);
    let id2 = registry.runtime_id_for_provider("provider-a");
    assert_eq!(id1.as_uuid().get_version_num(), 7);
    assert_eq!(id2.as_uuid().get_version_num(), 7);
}
