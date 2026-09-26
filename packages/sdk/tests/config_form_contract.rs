use sdk::*;

#[test]
fn config_form_wire_schema_preserves_identity_revision_and_typed_values() {
    let command = ConfigFormSubmitPage {
        session_id: ConfigFormSessionId("session-1".to_string()),
        expected_revision: ConfigFormRevision(4),
        values: vec![ConfigFormFieldValue {
            field_id: ConfigFormFieldId("provider_source".to_string()),
            value: ConfigFormValue::SelectedOption(ConfigFormOptionId("Anthropic".to_string())),
        }],
    };

    let json = serde_json::to_value(command).unwrap();
    assert_eq!(json["session_id"], "session-1");
    assert_eq!(json["expected_revision"], 4);
    assert_eq!(json["values"][0]["value"]["type"], "selected_option");
}

#[test]
fn config_form_view_never_contains_secret_text() {
    let view = ConfigFormView {
        workflow_id: ConfigFormWorkflowId("provider_connect".to_string()),
        session_id: ConfigFormSessionId("session-1".to_string()),
        revision: ConfigFormRevision(1),
        origin: ConfigFormOrigin::ExplicitCommand,
        page: ConfigFormPage {
            id: ConfigFormPageId("edit_credential".to_string()),
            title: "API Key".to_string(),
            description: None,
            step: None,
            fields: vec![ConfigFormField {
                id: ConfigFormFieldId("api_key".to_string()),
                label: "API Key".to_string(),
                description: None,
                field_type: ConfigFormFieldType::Secret,
                required: false,
                has_value: true,
                display_value: None,
                options: Vec::new(),
                error: None,
            }],
            error: None,
            actions: Vec::new(),
        },
        busy: None,
        terminal: None,
    };

    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("secret-key"));
    assert!(json.contains("has_value"));
    assert_eq!(serde_json::from_str::<ConfigFormView>(&json).unwrap(), view);
}

#[test]
fn config_form_origins_and_terminal_outcomes_are_distinct() {
    assert_ne!(
        ConfigFormOrigin::ExplicitCommand,
        ConfigFormOrigin::FirstChatBootstrap
    );
    assert_ne!(
        ConfigFormTerminal::Cancelled,
        ConfigFormTerminal::Completed {
            applied_revision: Some(7)
        }
    );
}
