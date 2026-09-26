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
        existing_providers: Vec::new(),
        available_actions: crate::connect::AvailableAction::for_stage(stage, None),
        probe_status: None,
        last_error: None,
        terminal: None,
    }
}

#[test]
fn provider_selection_lists_existing_custom_sources_before_custom_option() {
    // 全局配置已有但不在 catalog 的自定义 Provider（如 OmniRoute）必须
    // 出现在列表（覆盖路径），而不是强迫用户走"自定义"重新输入。
    let mut view = connect_view(ConnectStage::SelectProvider);
    view.existing_providers = vec![
        crate::connect::ExistingProviderSummary {
            source: "Zhipu".to_string(),
            driver: Some("zhipu".to_string()),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            api_key_status: crate::connect::ExistingCredentialStatus::Present,
            model_id: Some("glm-5.3".to_string()),
        },
        crate::connect::ExistingProviderSummary {
            source: "OmniRoute".to_string(),
            driver: Some("openai".to_string()),
            base_url: "https://genius.infra.wanaka.app".to_string(),
            api_key_status: crate::connect::ExistingCredentialStatus::Present,
            model_id: Some("cursor/auto".to_string()),
        },
    ];

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    let ids: Vec<&str> = form.page.fields[0]
        .options
        .iter()
        .map(|option| option.id.as_str())
        .collect();
    assert!(
        ids.contains(&"OmniRoute"),
        "自定义已有 source 必须出现在列表"
    );
    assert_eq!(
        ids.iter().filter(|id| **id == "Zhipu").count(),
        1,
        "catalog 内 source 不因快照重复追加"
    );
    assert_eq!(ids.last().copied(), Some("custom"));

    // 提交 OmniRoute（非 catalog）→ SelectProvider{source: new_owned}
    let command = connect_command_for_form(
        &view,
        ConfigFormCommand::SubmitPage {
            values: vec![ConfigFormFieldValue {
                field_id: ConfigFormFieldId::new("provider_source").unwrap(),
                value: ConfigFormValue::SelectedOption(
                    ConfigFormOptionId::new("OmniRoute").unwrap(),
                ),
            }],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    assert!(
        matches!(
            command,
            crate::connect::ConnectCommand::SelectProvider { source }
                if source.as_str() == "OmniRoute"
        ),
        "非 catalog 已有 source 必须映射为 SelectProvider"
    );
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
fn review_page_displays_every_chosen_configuration() {
    // 预览页必须显示全部已选配置：Provider / endpoint / key 掩码 /
    // 模型 / UA / 全局默认。
    let mut view = connect_view(ConnectStage::Review);
    view.draft.source = Some(
        crate::catalog::find_by_source("Zhipu Coding Plan")
            .unwrap()
            .source
            .clone(),
    );
    view.draft.base_url = Some("https://open.bigmodel.cn/api/coding/paas/v4".to_string());
    view.draft.has_api_key = true;
    view.draft.credential_mask = Some("sk-h****wxyz".to_string());
    view.draft.provider_user_agent = Some("ZCode/3.11.2".to_string());
    view.draft.models = vec![crate::connect::ModelDraftView {
        model_id: "glm-5.3".to_string(),
        context_window: Some(1_048_576),
        max_tokens: Some(16_384),
        reasoning_effort: None,
    }];
    view.draft.default_model_id = Some("glm-5.3".to_string());

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    let labels: Vec<&str> = form
        .page
        .fields
        .iter()
        .map(|field| field.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec![
            "Provider",
            "Base URL",
            "API Key",
            "模型（默认）",
            "User-Agent",
            "全局默认模型"
        ]
    );
    let values: Vec<&str> = form
        .page
        .fields
        .iter()
        .filter_map(|field| field.display_value.as_deref())
        .collect();
    assert_eq!(
        values,
        vec![
            "Zhipu Coding Plan",
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "sk-h****wxyz",
            "glm-5.3（Context 1048576 · Max 16384）",
            "ZCode/3.11.2",
            "glm-5.3",
        ]
    );
}

#[test]
fn endpoint_page_offers_api_style_for_openai_family_only() {
    // OpenAI 系 driver（zhipu/openai/deepseek 等）必须在 endpoint 页提供
    // 接口风格选择（Chat Completions / Responses）；anthropic / ollama
    // 不支持 Responses，不显示该字段。
    for (source, expects_style_field) in [
        ("OpenAI", true),
        ("Zhipu Coding Plan", true),
        ("DeepSeek", true),
        ("LiteLLM", true),
        ("Anthropic", false),
        ("Ollama", false),
    ] {
        let mut view = connect_view(ConnectStage::EditEndpoint);
        view.draft.source = Some(
            crate::catalog::find_by_source(source)
                .unwrap()
                .source
                .clone(),
        );

        let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

        let style_fields = form
            .page
            .fields
            .iter()
            .filter(|field| field.id.as_str() == "api_style")
            .count();
        assert_eq!(
            style_fields,
            usize::from(expects_style_field),
            "{source} 接口风格字段出现数不符"
        );
        if expects_style_field {
            let field = form
                .page
                .fields
                .iter()
                .find(|field| field.id.as_str() == "api_style")
                .unwrap();
            let labels: Vec<&str> = field
                .options
                .iter()
                .map(|option| option.label.as_str())
                .collect();
            assert_eq!(labels, vec!["Chat Completions", "Responses"]);
            assert!(field.has_value, "{source} 必须预选接口风格");
        }
    }
}

#[test]
fn endpoint_submission_carries_api_style_choice() {
    for (option_id, expected) in [("chat", None), ("responses", Some("responses"))] {
        let mut view = connect_view(ConnectStage::EditEndpoint);
        view.draft.source = Some(
            crate::catalog::find_by_source("OpenAI")
                .unwrap()
                .source
                .clone(),
        );
        let command = connect_command_for_form(
            &view,
            ConfigFormCommand::SubmitPage {
                values: vec![
                    ConfigFormFieldValue {
                        field_id: ConfigFormFieldId::new("base_url").unwrap(),
                        value: ConfigFormValue::Text("https://api.openai.com/v1".to_string()),
                    },
                    ConfigFormFieldValue {
                        field_id: ConfigFormFieldId::new("api_style").unwrap(),
                        value: ConfigFormValue::SelectedOption(
                            ConfigFormOptionId::new(option_id).unwrap(),
                        ),
                    },
                ],
            },
            crate::catalog::PROVIDER_CATALOG,
        )
        .unwrap();
        assert!(
            matches!(
                &command,
                crate::connect::ConnectCommand::SetEndpoint { base_url, api_style }
                    if base_url == "https://api.openai.com/v1"
                        && *api_style == expected.map(str::to_string)
            ),
            "选项 {option_id} 必须映射 api_style={expected:?}"
        );
    }
}

#[test]
fn provider_selection_ends_with_fully_custom_option() {
    // Provider 列表末尾必须有"自定义（完全自定义）"选项；选中后提交映射
    // BeginCustomProvider；EditCustomProvider 页提供名称/driver/endpoint 三字段。
    let form = provider_connect_form_view(
        &connect_view(ConnectStage::SelectProvider),
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    let last_option = form.page.fields[0].options.last().unwrap();
    assert_eq!(last_option.id.as_str(), "custom");
    assert_eq!(last_option.label, "自定义（完全自定义）");

    let command = connect_command_for_form(
        &connect_view(ConnectStage::SelectProvider),
        ConfigFormCommand::SubmitPage {
            values: vec![ConfigFormFieldValue {
                field_id: ConfigFormFieldId::new("provider_source").unwrap(),
                value: ConfigFormValue::SelectedOption(ConfigFormOptionId::new("custom").unwrap()),
            }],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    assert!(matches!(
        command,
        crate::connect::ConnectCommand::BeginCustomProvider
    ));

    let custom_form = provider_connect_form_view(
        &connect_view(ConnectStage::EditCustomProvider),
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    assert_eq!(custom_form.page.id.as_str(), "edit_custom_provider");
    let ids: Vec<&str> = custom_form
        .page
        .fields
        .iter()
        .map(|field| field.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["provider_name", "custom_driver", "custom_base_url"]
    );
}

#[test]
fn credential_page_displays_existing_key_mask_above_and_prefills_input() {
    // 已保留 key 时：字段上方显示掩码（display_value），输入框预填掩码
    // 原文（TUI Secret 按长度打点显示）；掩码原样提交 = 保留（空提交）。
    let mut view = connect_view(ConnectStage::EditCredential);
    view.draft.has_api_key = true;
    view.draft.credential_mask = Some("sk-h****wxyz".to_string());

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    let field = &form.page.fields[0];
    assert_eq!(field.display_value.as_deref(), Some("sk-h****wxyz"));
    assert!(field
        .description
        .as_deref()
        .is_some_and(|description| description.contains("sk-h****wxyz")));
    assert!(!format!("{form:?}").contains("plaintext-key"));

    let command = connect_command_for_form(
        &view,
        ConfigFormCommand::SubmitPage {
            values: vec![ConfigFormFieldValue {
                field_id: ConfigFormFieldId::new("api_key").unwrap(),
                value: ConfigFormValue::Secret("sk-h****wxyz".to_string()),
            }],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    assert!(
        matches!(
            command,
            crate::connect::ConnectCommand::SetCredential { api_key } if api_key.is_empty()
        ),
        "掩码原样提交必须归一为空提交以保留现有 key"
    );
}

#[test]
fn probing_page_submission_maps_to_continue_after_probe() {
    // 探测失败后回车提交必须映射为"继续"，而不是报
    // "Probing 页面不接受字段提交"导致表单退出。
    let command = connect_command_for_form(
        &connect_view(ConnectStage::Probing),
        ConfigFormCommand::SubmitPage { values: Vec::new() },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert!(
        matches!(command, crate::connect::ConnectCommand::ContinueAfterProbe),
        "Probing 页提交必须映射为 ContinueAfterProbe"
    );
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
        vec![
            "model_id",
            "context_window",
            "max_tokens",
            "reasoning_effort",
            "global_default",
        ]
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

    // Anthropic 发布 4 个当前在线模型（多选，无 custom 混入项）。
    assert!(model_options.len() >= 4);
    assert_eq!(model_options[0].id.as_str(), "recommended-claude-fable-5-1");
    assert_eq!(model_options[0].label, "claude-fable-5-1");
    assert_eq!(model_options[1].label, "claude-opus-5");
    assert_eq!(model_options[2].label, "claude-sonnet-5");
    assert_eq!(model_options[3].label, "claude-haiku-4-5");
}

#[test]
fn custom_model_page_prefills_first_selected_model_for_editing() {
    // 编辑场景：预填 draft 首个已选模型（含推理档位），用户改属性后保存
    // 即 upsert；添加场景用户直接覆盖输入。
    let mut view = connect_view(ConnectStage::EditCustomModel);
    view.draft.models = vec![crate::connect::ModelDraftView {
        model_id: "claude-fable-5-1".to_string(),
        context_window: Some(1_000_000),
        max_tokens: Some(131_072),
        reasoning_effort: Some("high".to_string()),
    }];

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert_eq!(
        form.page.fields[0].display_value.as_deref(),
        Some("claude-fable-5-1")
    );
    assert_eq!(
        form.page.fields[1].display_value.as_deref(),
        Some("1000000")
    );
    assert_eq!(form.page.fields[2].display_value.as_deref(), Some("131072"));
    assert_eq!(form.page.fields[3].display_value.as_deref(), Some("high"));
}

#[test]
fn zhipu_endpoint_pages_publish_distinct_default_urls() {
    for (source, expected_url) in [
        ("Zhipu", "https://open.bigmodel.cn/api/paas/v4"),
        (
            "Zhipu Coding Plan",
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ),
    ] {
        let mut view = connect_view(ConnectStage::EditEndpoint);
        view.draft.source = Some(
            crate::catalog::find_by_source(source)
                .unwrap()
                .source
                .clone(),
        );

        let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

        assert_eq!(
            form.page.fields[0].display_value.as_deref(),
            Some(expected_url),
            "{source} 必须显示自己的内置 endpoint"
        );
    }
}

#[test]
fn zhipu_user_agent_page_prefills_official_sdk_client_ua() {
    // zhipu 系（含 Z.ai）有已核验的官方客户端 UA 时，表单必须预填，
    // 用户不应手动输入；LiteLLM 无证据时保持空，交由全局默认回退。
    for (source, expected_ua) in [
        ("Zhipu", Some("ZCode/3.11.2")),
        ("Zhipu Coding Plan", Some("ZCode/3.11.2")),
        ("Z.ai", Some("ZCode/3.11.2")),
        ("Z.ai Coding Plan", Some("ZCode/3.11.2")),
        ("Anthropic", Some("claude-cli/2.1.267 (external, sdk-cli)")),
        ("LiteLLM", None),
    ] {
        let mut view = connect_view(ConnectStage::EditUserAgent);
        view.draft.source = Some(
            crate::catalog::find_by_source(source)
                .unwrap()
                .source
                .clone(),
        );

        let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

        assert_eq!(
            form.page.fields[0].display_value.as_deref(),
            expected_ua,
            "{source} 官方客户端 UA 预填不符"
        );
    }
}

#[test]
fn model_select_page_marks_existing_draft_model_as_selected() {
    // 确认覆盖已有 Provider 后 draft.model 来自全局配置；SelectModel 页必须
    // 把该模型标记为已选（has_value + display_value），供 TUI 预选 option。
    let mut view = connect_view(ConnectStage::SelectModel);
    view.draft.source = Some(
        crate::catalog::find_by_source("Zhipu")
            .unwrap()
            .source
            .clone(),
    );
    view.draft.models = vec![crate::connect::ModelDraftView {
        model_id: "glm-5.2".to_string(),
        context_window: Some(204_800),
        max_tokens: Some(16_000),
        reasoning_effort: None,
    }];

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    let field = &form.page.fields[0];
    assert!(field.has_value, "draft 已有模型时必须标记 has_value");
    assert_eq!(field.display_value.as_deref(), Some("glm-5.2"));
}

#[test]
fn custom_model_page_keeps_fields_empty_without_catalog_defaults() {
    let mut view = connect_view(ConnectStage::EditCustomModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("LiteLLM"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();

    assert!(form.page.fields[0].display_value.is_none());
    assert!(form.page.fields[1].display_value.is_none());
    // global_default 始终预选"否"；max_tokens 预填全局默认 8192；
    // 其余字段无 catalog 默认时保持空。
    for field in &form.page.fields {
        match field.id.as_str() {
            "global_default" => assert_eq!(field.display_value.as_deref(), Some("否")),
            "max_tokens" => assert_eq!(field.display_value.as_deref(), Some("8192")),
            _ => assert!(!field.has_value),
        }
    }
}

#[test]
fn minimax_model_page_publishes_128k_output_cap() {
    // MiniMax 官方只公布上下文窗口；max output 按产品决策统一 128K。
    let mut view = connect_view(ConnectStage::SelectModel);
    view.draft.source = Some(crate::catalog::ProviderSource::new("Minimax"));

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();
    let model_options = &form.page.fields[0].options;

    assert_eq!(model_options[0].label, "MiniMax-M3");
    assert_eq!(
        model_options[0].description.as_deref(),
        Some("Context 1000000 · Max 131072")
    );
    assert_eq!(model_options[1].label, "MiniMax-M2.7");
    assert_eq!(
        model_options[1].description.as_deref(),
        Some("Context 204800 · Max 131072")
    );
    assert_eq!(model_options[2].label, "MiniMax-M2.5");
    assert_eq!(
        model_options[2].description.as_deref(),
        Some("Context 204800 · Max 131072")
    );
    assert_eq!(model_options[3].label, "MiniMax-M2.1");
    assert_eq!(
        model_options[3].description.as_deref(),
        Some("Context 204800 · Max 131072")
    );
}

#[test]
fn model_page_multi_select_prefills_configured_models_and_keeps_custom() {
    // 模型页契约：MultiSelect；推荐 ∪ 已配置（推荐外自定义也带上）；
    // 已配置默认勾选（display_value 预选串）；提交完整集合。
    let mut view = connect_view(ConnectStage::SelectModel);
    view.draft.source = Some(
        crate::catalog::find_by_source("DeepSeek")
            .unwrap()
            .source
            .clone(),
    );
    view.draft.models = vec![
        crate::connect::ModelDraftView {
            model_id: "deepseek-v4-pro".to_string(),
            context_window: Some(1_048_576),
            max_tokens: Some(16_384),
            reasoning_effort: Some("high".to_string()),
        },
        crate::connect::ModelDraftView {
            model_id: "my-private-model".to_string(),
            context_window: Some(128_000),
            max_tokens: Some(8_192),
            reasoning_effort: None,
        },
    ];

    let form = provider_connect_form_view(&view, crate::catalog::PROVIDER_CATALOG).unwrap();
    let field = &form.page.fields[0];
    assert_eq!(field.field_type, ConfigFormFieldType::MultiSelect);
    let labels: Vec<&str> = field
        .options
        .iter()
        .map(|option| option.label.as_str())
        .collect();
    assert!(
        labels.contains(&"deepseek-v4-pro") && labels.contains(&"deepseek-flash"),
        "推荐模型必须在列表"
    );
    assert!(
        labels.contains(&"my-private-model"),
        "推荐外的自定义已配置模型必须带上"
    );
    assert_eq!(
        field.display_value.as_deref(),
        Some("deepseek-v4-pro, my-private-model"),
        "已配置模型必须默认选中"
    );

    let command = connect_command_for_form(
        &view,
        ConfigFormCommand::SubmitPage {
            values: vec![ConfigFormFieldValue {
                field_id: ConfigFormFieldId::new("recommended_models").unwrap(),
                value: ConfigFormValue::SelectedOptions(vec![
                    ConfigFormOptionId::new("recommended-deepseek-v4-pro").unwrap(),
                    ConfigFormOptionId::new("configured-my-private-model").unwrap(),
                ]),
            }],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();
    match command {
        crate::connect::ConnectCommand::SetSelectedModels { models } => {
            assert_eq!(models.len(), 2);
            assert_eq!(models[0].model_id, "deepseek-v4-pro");
            assert_eq!(models[1].model_id, "my-private-model");
            assert_eq!(models[1].context_window, 128_000);
        }
        other => panic!("必须映射 SetSelectedModels，得到 {other:?}"),
    }
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
                ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("reasoning_effort").unwrap(),
                    value: ConfigFormValue::SelectedOption(
                        ConfigFormOptionId::new("high").unwrap(),
                    ),
                },
                ConfigFormFieldValue {
                    field_id: ConfigFormFieldId::new("global_default").unwrap(),
                    value: ConfigFormValue::SelectedOption(ConfigFormOptionId::new("yes").unwrap()),
                },
            ],
        },
        crate::catalog::PROVIDER_CATALOG,
    )
    .unwrap();

    assert!(matches!(
        command,
        crate::connect::ConnectCommand::UpsertCustomModel { model, set_as_default: true }
            if model.model_id == "custom-model"
                && model.context_window == 128_000
                && model.max_tokens == 8_192
                && model.reasoning_effort.as_deref() == Some("high")
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
