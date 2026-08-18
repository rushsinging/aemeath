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
fn endpoint_page_prefills_catalog_default_url() {
    let mut view = connect_view(ConnectStage::EditEndpoint);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Anthropic"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert_eq!(
        form.page.fields[0].display_value.as_deref(),
        Some("https://api.anthropic.com")
    );
    assert!(form.page.fields[0].has_value);
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
fn model_page_publishes_every_catalog_model_before_custom_option() {
    let mut view = connect_view(ConnectStage::SelectModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Anthropic"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();
    let model_options = &form.page.fields[0].options;

    assert!(model_options.len() >= 3);
    assert_eq!(model_options[0].id.as_str(), "recommended-0");
    assert_eq!(model_options[0].label, "claude-opus-4-1-20250805");
    assert_eq!(model_options[1].id.as_str(), "recommended-1");
    assert_eq!(model_options[1].label, "claude-sonnet-4-20250514");
    assert_eq!(model_options.last().unwrap().id.as_str(), "custom");
}

#[test]
fn custom_model_page_prefills_first_catalog_model_defaults() {
    let mut view = connect_view(ConnectStage::EditCustomModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Anthropic"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert_eq!(
        form.page.fields[0].display_value.as_deref(),
        Some("claude-opus-4-1-20250805")
    );
    assert_eq!(form.page.fields[1].display_value.as_deref(), Some("200000"));
    assert_eq!(form.page.fields[2].display_value.as_deref(), Some("32000"));
    assert!(form.page.fields.iter().all(|field| field.has_value));
}

#[test]
fn zhipu_endpoint_pages_publish_distinct_default_urls() {
    for (source, expected_url) in [
        ("Zhipu", "https://open.bigmodel.cn/api/paas/v4"),
        (
            "ZhipuCodingPlan",
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ),
    ] {
        let mut view = connect_view(ConnectStage::EditEndpoint);
        view.draft.source = Some(crate::catalog::find_by_source(source).unwrap().source);

        let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

        assert_eq!(
            form.page.fields[0].display_value.as_deref(),
            Some(expected_url),
            "{source} 必须显示自己的内置 endpoint"
        );
    }
}

#[test]
fn custom_model_page_keeps_fields_empty_without_catalog_defaults() {
    let mut view = connect_view(ConnectStage::EditCustomModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("LiteLLM"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert!(form.page.fields[0].display_value.is_none());
    assert!(form.page.fields[1].display_value.is_none());
    assert!(form.page.fields[2].display_value.is_none());
    assert!(form.page.fields.iter().all(|field| !field.has_value));
}

#[test]
fn model_page_marks_unpublished_output_cap_instead_of_zero() {
    let mut view = connect_view(ConnectStage::SelectModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Minimax"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();
    let model_options = &form.page.fields[0].options;

    assert_eq!(model_options[0].label, "MiniMax-M3");
    assert_eq!(
        model_options[0].description.as_deref(),
        Some("Context 1000000 · Max 官方未公布")
    );
    assert_eq!(model_options[1].label, "MiniMax-M2.7");
    assert_eq!(
        model_options[1].description.as_deref(),
        Some("Context 204800 · Max 官方未公布")
    );
}

#[test]
fn custom_model_page_leaves_max_tokens_empty_when_catalog_value_unpublished() {
    let mut view = connect_view(ConnectStage::EditCustomModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Minimax"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert_eq!(
        form.page.fields[0].display_value.as_deref(),
        Some("MiniMax-M3")
    );
    assert_eq!(
        form.page.fields[1].display_value.as_deref(),
        Some("1000000")
    );
    assert!(
        form.page.fields[2].display_value.is_none(),
        "官方未公布输出上限时 Max Tokens 不得预填 0"
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
