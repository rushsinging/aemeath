use serde_json::Value;

#[test]
fn wire_components_document_exposes_only_pure_wire_contracts() {
    let document = sdk::wire::components_document();
    let schemas = document["$defs"]
        .as_object()
        .expect("$defs must be an object");

    assert_eq!(
        document["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(schemas.contains_key("InteractionRequest"));
    assert!(schemas.contains_key("ConfigUpdate"));
    assert!(schemas.contains_key("ConfigView"));
    assert!(schemas.contains_key("ProjectContext"));
    assert!(schemas.contains_key("ModelSummary"));
    assert!(schemas.contains_key("ReflectionHistoryView"));
    assert!(schemas.contains_key("ChatEventContext"));
    assert!(schemas.contains_key("SessionSummary"));
    assert!(schemas.contains_key("SessionSnapshot"));
    assert!(schemas.contains_key("ChatMessage"));
    assert!(schemas.contains_key("WorkspaceContextView"));
    assert!(schemas.contains_key("HookMessageView"));
    assert!(schemas.contains_key("SessionResumeFailureKind"));
    assert!(!schemas.contains_key("AgentClient"));
    assert!(!schemas.contains_key("ChatStream"));
}

#[test]
fn wire_components_document_contains_no_transport_operations() {
    let document = sdk::wire::components_document();

    assert!(document.get("paths").is_none());
    assert!(document.get("servers").is_none());
    assert!(document.get("openapi").is_none());
}

#[test]
fn wire_components_document_preserves_interaction_and_run_control_references() {
    let document = sdk::wire::components_document();
    let schemas = document["$defs"]
        .as_object()
        .expect("$defs must be an object");

    assert_eq!(
        schemas["InteractionRequest"]["properties"]["id"]["$ref"],
        "#/$defs/InteractionRequestId"
    );
    assert_eq!(
        schemas["InteractionRequest"]["properties"]["run_id"]["$ref"],
        "#/$defs/RunId"
    );
    assert_eq!(
        schemas["ControlDeadline"]["properties"]["unix_millis"]["type"],
        "integer"
    );
    assert_eq!(
        schemas["InteractionRequestBody"]["oneOf"]
            .as_array()
            .map(Vec::len),
        Some(4)
    );

    let _: &Value = &document;
}
