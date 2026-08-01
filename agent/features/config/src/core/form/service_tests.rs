use super::*;

fn sample_page() -> ConfigFormPage {
    ConfigFormPage {
        id: ConfigFormPageId::new("select_provider").unwrap(),
        title: "选择 Provider".to_string(),
        description: None,
        step: Some(ConfigFormStep {
            current: 1,
            total: 2,
        }),
        fields: vec![ConfigFormField {
            id: ConfigFormFieldId::new("provider_source").unwrap(),
            label: "Provider".to_string(),
            description: None,
            field_type: ConfigFormFieldType::SingleSelect,
            required: true,
            has_value: false,
            display_value: None,
            options: vec![ConfigFormOption {
                id: ConfigFormOptionId::new("anthropic").unwrap(),
                label: "Anthropic".to_string(),
                description: None,
            }],
            error: None,
        }],
        error: None,
        actions: vec![ConfigFormAction {
            id: ConfigFormActionId::new("submit").unwrap(),
            label: "继续".to_string(),
            style: ConfigFormActionStyle::Primary,
            shortcut: Some("Enter".to_string()),
        }],
    }
}

#[test]
fn identifiers_reject_blank_values() {
    assert!(ConfigFormWorkflowId::new("  ").is_err());
    assert!(ConfigFormFieldId::new("").is_err());
    assert!(ConfigFormActionId::new("\t").is_err());
}

#[test]
fn page_submission_rejects_stale_revision_without_mutating_session() {
    let workflow_id = ConfigFormWorkflowId::new("provider_connect").unwrap();
    let mut service = ConfigFormService::new();
    let initial = service.start(
        workflow_id,
        ConfigFormOrigin::ExplicitCommand,
        sample_page(),
    );

    let error = service
        .apply(
            initial.session_id.clone(),
            ConfigFormRevision::new(initial.revision.value() + 1),
            ConfigFormCommand::SubmitPage {
                values: vec![ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("provider_source").unwrap(),
                    value: ConfigFormValue::SelectedOption(
                        ConfigFormOptionId::new("anthropic").unwrap(),
                    ),
                }],
            },
        )
        .unwrap_err();

    assert!(matches!(error, ConfigFormError::StaleRevision { .. }));
    assert_eq!(
        service.view(&initial.session_id).unwrap().revision,
        initial.revision
    );
}

#[test]
fn invalid_page_submission_is_atomic() {
    let workflow_id = ConfigFormWorkflowId::new("provider_connect").unwrap();
    let mut service = ConfigFormService::new();
    let initial = service.start(
        workflow_id,
        ConfigFormOrigin::ExplicitCommand,
        sample_page(),
    );

    let error = service
        .apply(
            initial.session_id.clone(),
            initial.revision,
            ConfigFormCommand::SubmitPage {
                values: vec![ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("provider_source").unwrap(),
                    value: ConfigFormValue::Text("anthropic".to_string()),
                }],
            },
        )
        .unwrap_err();

    assert!(matches!(error, ConfigFormError::InvalidValueType { .. }));
    assert_eq!(service.view(&initial.session_id).unwrap(), initial);
}

#[test]
fn duplicate_and_missing_fields_leave_session_unchanged() {
    let workflow_id = ConfigFormWorkflowId::new("provider_connect").unwrap();
    let mut service = ConfigFormService::new();
    let initial = service.start(
        workflow_id,
        ConfigFormOrigin::ExplicitCommand,
        sample_page(),
    );
    let provider_field_id = ConfigFormFieldId::new("provider_source").unwrap();
    let selected_provider = ConfigFormFieldValue {
        field_id: provider_field_id.clone(),
        value: ConfigFormValue::SelectedOption(ConfigFormOptionId::new("anthropic").unwrap()),
    };

    let duplicate_error = service
        .apply(
            initial.session_id.clone(),
            initial.revision,
            ConfigFormCommand::SubmitPage {
                values: vec![selected_provider.clone(), selected_provider],
            },
        )
        .unwrap_err();
    assert!(matches!(
        duplicate_error,
        ConfigFormError::DuplicateField { field_id } if field_id == provider_field_id
    ));
    assert_eq!(service.view(&initial.session_id).unwrap(), initial);

    let missing_error = service
        .apply(
            initial.session_id.clone(),
            initial.revision,
            ConfigFormCommand::SubmitPage { values: Vec::new() },
        )
        .unwrap_err();
    assert!(matches!(
        missing_error,
        ConfigFormError::MissingRequiredField { field_id } if field_id == provider_field_id
    ));
    assert_eq!(service.view(&initial.session_id).unwrap(), initial);
}

#[test]
fn unknown_action_does_not_advance_revision() {
    let workflow_id = ConfigFormWorkflowId::new("provider_connect").unwrap();
    let mut service = ConfigFormService::new();
    let initial = service.start(
        workflow_id,
        ConfigFormOrigin::ExplicitCommand,
        sample_page(),
    );

    let error = service
        .apply(
            initial.session_id.clone(),
            initial.revision,
            ConfigFormCommand::InvokeAction {
                action_id: ConfigFormActionId::new("unknown").unwrap(),
            },
        )
        .unwrap_err();

    assert!(matches!(error, ConfigFormError::UnknownAction { .. }));
    assert_eq!(service.view(&initial.session_id).unwrap(), initial);
}

#[test]
fn successful_submission_advances_revision_and_keeps_secret_out_of_view() {
    let workflow_id = ConfigFormWorkflowId::new("provider_connect").unwrap();
    let mut service = ConfigFormService::new();
    let initial = service.start(
        workflow_id,
        ConfigFormOrigin::ExplicitCommand,
        ConfigFormPage {
            id: ConfigFormPageId::new("credential").unwrap(),
            title: "API Key".to_string(),
            description: None,
            step: None,
            fields: vec![ConfigFormField {
                id: ConfigFormFieldId::new("api_key").unwrap(),
                label: "API Key".to_string(),
                description: None,
                field_type: ConfigFormFieldType::Secret,
                required: false,
                has_value: false,
                display_value: None,
                options: Vec::new(),
                error: None,
            }],
            error: None,
            actions: Vec::new(),
        },
    );

    let updated = service
        .apply(
            initial.session_id.clone(),
            initial.revision,
            ConfigFormCommand::SubmitPage {
                values: vec![ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("api_key").unwrap(),
                    value: ConfigFormValue::Secret("secret-key".to_string()),
                }],
            },
        )
        .unwrap();

    assert_eq!(updated.revision.value(), initial.revision.value() + 1);
    assert!(updated.page.fields[0].has_value);
    assert_eq!(updated.page.fields[0].display_value, None);
    assert!(!format!("{updated:?}").contains("secret-key"));
}

#[test]
fn command_names_do_not_expose_secret_payloads() {
    let command = ConfigFormCommand::SubmitPage {
        values: vec![ConfigFormFieldValue {
            field_id: ConfigFormFieldId::new("api_key").unwrap(),
            value: ConfigFormValue::Secret("secret-key".to_string()),
        }],
    };

    assert_eq!(format!("{command:?}"), "SubmitPage { field_count: 1 }");
}
