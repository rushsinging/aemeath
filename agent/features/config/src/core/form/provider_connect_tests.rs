use super::*;
use crate::connect::{
    ConnectDraftView, ConnectOrigin, ConnectRevision, ConnectSessionId, ConnectStage, ConnectView,
};

fn connect_view(stage: ConnectStage) -> ConnectView {
    ConnectView {
        session_id: ConnectSessionId::from_transport_str("018f47a2-48d8-7b58-8f6e-9d34223df8bd")
            .unwrap(),
        revision: ConnectRevision::from_value(3),
        stage,
        origin: ConnectOrigin::ExplicitCommand,
        draft: ConnectDraftView::default(),
        existing_provider: None,
        available_actions: crate::connect::AvailableAction::for_stage(stage, None),
        probe_status: None,
        last_error: None,
        terminal: None,
    }
}

#[test]
fn select_provider_page_publishes_catalog_options_and_stable_ids() {
    let form = provider_connect_form_view(
        &connect_view(ConnectStage::SelectProvider),
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert_eq!(form.workflow_id.as_str(), PROVIDER_CONNECT_WORKFLOW_ID);
    assert_eq!(form.page.id.as_str(), "select_provider");
    assert_eq!(form.page.fields[0].id.as_str(), "provider_source");
    assert!(form.page.fields[0]
        .options
        .iter()
        .any(|option| option.id.as_str() == "Anthropic"));
}

#[test]
fn credential_page_is_secret_and_never_contains_plaintext() {
    let mut view = connect_view(ConnectStage::EditCredential);
    view.draft.has_api_key = true;
    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert_eq!(form.page.id.as_str(), "edit_credential");
    assert_eq!(form.page.fields[0].field_type, ConfigFormFieldType::Secret);
    assert!(form.page.fields[0].has_value);
    assert_eq!(form.page.fields[0].display_value, None);
}

#[test]
fn typed_provider_selection_maps_to_connect_command() {
    let command = connect_command_for_form(
        &connect_view(ConnectStage::SelectProvider),
        ConfigFormCommand::SubmitPage {
            values: vec![ConfigFormFieldValue {
                field_id: ConfigFormFieldId::new("provider_source").unwrap(),
                value: ConfigFormValue::SelectedOption(
                    ConfigFormOptionId::new("Anthropic").unwrap(),
                ),
            }],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert!(matches!(
        command,
        crate::connect::ConnectCommand::SelectProvider { source }
            if source.as_str() == "Anthropic"
    ));
}

#[test]
fn back_form_command_maps_to_connect_back_command() {
    let command = connect_command_for_form(
        &connect_view(ConnectStage::EditEndpoint),
        ConfigFormCommand::Back,
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert!(matches!(command, crate::connect::ConnectCommand::Back));
}

#[test]
fn custom_model_page_requires_three_typed_fields() {
    let form = provider_connect_form_view(
        &connect_view(ConnectStage::EditCustomModel),
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert_eq!(
        form.page
            .fields
            .iter()
            .map(|field| field.id.as_str())
            .collect::<Vec<_>>(),
        vec!["model_id", "context_window", "max_tokens"]
    );
}

#[test]
fn confirm_overwrite_page_exposes_both_server_actions() {
    let form = provider_connect_form_view(
        &connect_view(ConnectStage::ConfirmOverwrite),
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert_eq!(
        form.page
            .actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec!["confirm_overwrite", "reject_overwrite", "cancel"]
    );
}

#[test]
fn custom_model_submission_maps_all_typed_fields() {
    let command = connect_command_for_form(
        &connect_view(ConnectStage::EditCustomModel),
        ConfigFormCommand::SubmitPage {
            values: vec![
                ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("model_id").unwrap(),
                    value: ConfigFormValue::Text("custom-model".to_string()),
                },
                ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("context_window").unwrap(),
                    value: ConfigFormValue::Number(128_000),
                },
                ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("max_tokens").unwrap(),
                    value: ConfigFormValue::Number(8_192),
                },
            ],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert!(matches!(
        command,
        crate::connect::ConnectCommand::SetCustomModel {
            model_id,
            context_window: 128_000,
            max_tokens: 8_192,
        } if model_id == "custom-model"
    ));
}

#[test]
fn all_connect_stages_publish_a_form_page_or_terminal() {
    let stages = [
        ConnectStage::SelectProvider,
        ConnectStage::ConfirmOverwrite,
        ConnectStage::EditEndpoint,
        ConnectStage::EditCredential,
        ConnectStage::EditUserAgent,
        ConnectStage::SelectModel,
        ConnectStage::EditCustomModel,
        ConnectStage::ChooseGlobalDefault,
        ConnectStage::ChooseProbe,
        ConnectStage::Probing,
        ConnectStage::Review,
        ConnectStage::Saving,
        ConnectStage::Completed,
        ConnectStage::Cancelled,
    ];

    for stage in stages {
        let mut view = connect_view(stage);
        view.terminal = match stage {
            ConnectStage::Completed => Some(crate::connect::ConnectOutcome::Completed {
                applied_revision: 9,
            }),
            ConnectStage::Cancelled => Some(crate::connect::ConnectOutcome::Cancelled),
            _ => None,
        };
        let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();
        assert!(!form.page.id.as_str().is_empty());
        assert_eq!(form.terminal.is_some(), stage.is_terminal());
    }
}
