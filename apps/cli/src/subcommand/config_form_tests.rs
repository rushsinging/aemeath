use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn secret_view() -> sdk::ConfigFormView {
    sdk::ConfigFormView {
        workflow_id: sdk::ConfigFormWorkflowId("provider_connect".to_string()),
        session_id: sdk::ConfigFormSessionId("session-1".to_string()),
        revision: sdk::ConfigFormRevision(1),
        origin: sdk::ConfigFormOrigin::ExplicitCommand,
        page: sdk::ConfigFormPage {
            id: sdk::ConfigFormPageId("edit_credential".to_string()),
            title: "API Key".to_string(),
            description: None,
            step: None,
            fields: vec![sdk::ConfigFormField {
                id: sdk::ConfigFormFieldId("api_key".to_string()),
                label: "API Key".to_string(),
                description: None,
                field_type: sdk::ConfigFormFieldType::Secret,
                required: false,
                has_value: false,
                display_value: None,
                options: Vec::new(),
                error: None,
            }],
            error: None,
            actions: Vec::new(),
        },
        busy: None,
        terminal: None,
    }
}

#[test]
fn secret_input_masks_render_value_and_emits_typed_effect() {
    let mut model = ConfigFormModel::new(secret_view());
    model.update(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(model.visible_input(), "•");

    let effect = model
        .update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        effect,
        ConfigFormEffect::SubmitPage { command }
            if command.expected_revision == sdk::ConfigFormRevision(1)
                && matches!(&command.values[0].value, sdk::ConfigFormValue::Secret(value) if value == "s")
    ));
}

#[test]
fn view_replacement_clears_secret_and_uses_server_revision() {
    let mut model = ConfigFormModel::new(secret_view());
    model.update(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let mut next = secret_view();
    next.revision = sdk::ConfigFormRevision(2);

    model.replace_view(next);

    assert_eq!(model.visible_input(), "");
    assert_eq!(model.view().revision, sdk::ConfigFormRevision(2));
}

fn select_page_view_with_selected_model() -> sdk::ConfigFormView {
    let mut view = secret_view();
    view.page.id = sdk::ConfigFormPageId("select_model".to_string());
    view.page.fields = vec![sdk::ConfigFormField {
        id: sdk::ConfigFormFieldId("recommended_model".to_string()),
        label: "模型".to_string(),
        description: None,
        field_type: sdk::ConfigFormFieldType::SingleSelect,
        required: true,
        has_value: true,
        display_value: Some("glm-5.2".to_string()),
        options: vec![
            sdk::ConfigFormOption {
                id: sdk::ConfigFormOptionId("recommended-0".to_string()),
                label: "glm-5.3".to_string(),
                description: None,
            },
            sdk::ConfigFormOption {
                id: sdk::ConfigFormOptionId("recommended-1".to_string()),
                label: "glm-5.2".to_string(),
                description: None,
            },
            sdk::ConfigFormOption {
                id: sdk::ConfigFormOptionId("custom".to_string()),
                label: "自定义模型".to_string(),
                description: None,
            },
        ],
        error: None,
    }];
    view
}

#[test]
fn summary_only_page_enter_invokes_focused_action() {
    // ConfirmOverwrite 等页只有 Summary 字段 + actions：回车必须触发
    // action（如"覆盖"），而不是提交空字段集报"页面不接受字段提交"。
    let mut view = secret_view();
    view.page.id = sdk::ConfigFormPageId("confirm_overwrite".to_string());
    view.page.fields = vec![sdk::ConfigFormField {
        id: sdk::ConfigFormFieldId("existing_provider".to_string()),
        label: "现有 Provider".to_string(),
        description: None,
        field_type: sdk::ConfigFormFieldType::Summary,
        required: true,
        has_value: true,
        display_value: Some("Zhipu Coding Plan".to_string()),
        options: Vec::new(),
        error: None,
    }];
    view.page.actions = vec![sdk::ConfigFormAction {
        id: sdk::ConfigFormActionId("confirm_overwrite".to_string()),
        label: "覆盖".to_string(),
        style: sdk::ConfigFormActionStyle::Primary,
        shortcut: None,
    }];

    let mut model = ConfigFormModel::new(view);

    let effect = model.update(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    assert!(
        matches!(
            effect,
            Some(ConfigFormEffect::InvokeAction { command })
                if command.action_id.as_str() == "confirm_overwrite"
        ),
        "回车必须触发 InvokeAction(覆盖)"
    );
}

#[test]
fn actions_page_enter_triggers_primary_action_by_default() {
    // ChooseProbe 页 actions [跳过测试(secondary), 测试连接(primary), 取消]：
    // 回车必须默认触发 primary（测试连接），而不是第一个（跳过测试）——
    // 否则用户以为在测试，实际跳过导致"没有显示结果"。
    let mut view = secret_view();
    view.page.id = sdk::ConfigFormPageId("choose_probe".to_string());
    view.page.fields = Vec::new();
    view.page.actions = vec![
        sdk::ConfigFormAction {
            id: sdk::ConfigFormActionId("skip_probe".to_string()),
            label: "跳过测试".to_string(),
            style: sdk::ConfigFormActionStyle::Secondary,
            shortcut: None,
        },
        sdk::ConfigFormAction {
            id: sdk::ConfigFormActionId("begin_probe".to_string()),
            label: "测试连接".to_string(),
            style: sdk::ConfigFormActionStyle::Primary,
            shortcut: None,
        },
        sdk::ConfigFormAction {
            id: sdk::ConfigFormActionId("cancel".to_string()),
            label: "取消".to_string(),
            style: sdk::ConfigFormActionStyle::Destructive,
            shortcut: None,
        },
    ];

    let mut model = ConfigFormModel::new(view);

    let effect = model.update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(
            effect,
            Some(ConfigFormEffect::InvokeAction { command })
                if command.action_id.as_str() == "begin_probe"
        ),
        "回车必须默认触发 primary action（测试连接）"
    );
}

#[test]
fn select_page_preselects_option_matching_display_value() {
    // 已有配置预填的模型（has_value + display_value）在进入 select 页时
    // 必须直接高亮对应 option，而不是从第一项开始。
    let mut model = ConfigFormModel::new(secret_view());

    model.replace_view(select_page_view_with_selected_model());

    assert_eq!(model.interaction().selected_option, 1);
}

#[test]
fn select_page_without_selected_value_starts_from_first_option() {
    let mut view = select_page_view_with_selected_model();
    view.page.fields[0].has_value = false;
    view.page.fields[0].display_value = None;

    let mut model = ConfigFormModel::new(secret_view());

    model.replace_view(view);

    assert_eq!(model.interaction().selected_option, 0);
}

#[test]
fn secret_field_prefills_mask_and_renders_as_dots() {
    // 已保留 key 的掩码预填输入框（原文作为待编辑值），
    // visible_input 按长度以密码格式显示。
    let mut view = secret_view();
    view.page.id = sdk::ConfigFormPageId("edit_credential".to_string());
    view.page.fields[0].field_type = sdk::ConfigFormFieldType::Secret;
    view.page.fields[0].display_value = Some("sk-h****wxyz".to_string());

    let model = ConfigFormModel::new(view);

    assert_eq!(model.visible_input(), "•".repeat("sk-h****wxyz".len()));
}

fn custom_model_view() -> sdk::ConfigFormView {
    let mut view = secret_view();
    view.page.id = sdk::ConfigFormPageId("edit_custom_model".to_string());
    view.page.fields = vec![
        sdk::ConfigFormField {
            id: sdk::ConfigFormFieldId("model_id".to_string()),
            label: "Model ID".to_string(),
            description: None,
            field_type: sdk::ConfigFormFieldType::Text,
            required: true,
            has_value: false,
            display_value: None,
            options: Vec::new(),
            error: None,
        },
        sdk::ConfigFormField {
            id: sdk::ConfigFormFieldId("context_window".to_string()),
            label: "Context Window".to_string(),
            description: None,
            field_type: sdk::ConfigFormFieldType::Number,
            required: true,
            has_value: false,
            display_value: None,
            options: Vec::new(),
            error: None,
        },
        sdk::ConfigFormField {
            id: sdk::ConfigFormFieldId("max_tokens".to_string()),
            label: "Max Tokens".to_string(),
            description: None,
            field_type: sdk::ConfigFormFieldType::Number,
            required: true,
            has_value: false,
            display_value: None,
            options: Vec::new(),
            error: None,
        },
    ];
    view
}

#[test]
fn model_id_enter_submits_page_without_advancing_focus() {
    let mut view = custom_model_view();
    view.page.fields[1].display_value = Some("128000".to_string());
    view.page.fields[2].display_value = Some("4096".to_string());
    let mut model = ConfigFormModel::new(view);
    for character in "custom-model".chars() {
        model.update(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let effect = model
        .update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("Model ID 输入后 Enter 应提交页面");

    assert_eq!(model.interaction().focused_field, 0);
    assert!(matches!(
        effect,
        ConfigFormEffect::SubmitPage { command }
            if matches!(&command.values[0].value, sdk::ConfigFormValue::Text(value) if value == "custom-model")
                && matches!(&command.values[1].value, sdk::ConfigFormValue::Number(value) if *value == 128_000)
                && matches!(&command.values[2].value, sdk::ConfigFormValue::Number(value) if *value == 4_096)
    ));
}

fn select_view() -> sdk::ConfigFormView {
    let mut view = secret_view();
    view.page.fields[0] = sdk::ConfigFormField {
        id: sdk::ConfigFormFieldId("provider_source".to_string()),
        label: "Provider".to_string(),
        description: None,
        field_type: sdk::ConfigFormFieldType::SingleSelect,
        required: true,
        has_value: false,
        display_value: None,
        options: vec![
            sdk::ConfigFormOption {
                id: sdk::ConfigFormOptionId("Anthropic".to_string()),
                label: "Anthropic".to_string(),
                description: None,
            },
            sdk::ConfigFormOption {
                id: sdk::ConfigFormOptionId("OpenAI".to_string()),
                label: "OpenAI".to_string(),
                description: None,
            },
        ],
        error: None,
    };
    view
}

fn action_view() -> sdk::ConfigFormView {
    let mut view = secret_view();
    view.page.fields.clear();
    view.page.actions = vec![
        sdk::ConfigFormAction {
            id: sdk::ConfigFormActionId("confirm".to_string()),
            label: "确认保存".to_string(),
            style: sdk::ConfigFormActionStyle::Primary,
            shortcut: None,
        },
        sdk::ConfigFormAction {
            id: sdk::ConfigFormActionId("cancel".to_string()),
            label: "取消".to_string(),
            style: sdk::ConfigFormActionStyle::Destructive,
            shortcut: Some("Esc".to_string()),
        },
    ];
    view
}

#[test]
fn single_select_key_changes_interaction_and_submission_value() {
    let mut model = ConfigFormModel::new(select_view());
    model.update(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(model.interaction().selected_option, 1);
    let effect = model
        .update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        effect,
        ConfigFormEffect::SubmitPage { command }
            if matches!(
                &command.values[0].value,
                sdk::ConfigFormValue::SelectedOption(option) if option.as_str() == "OpenAI"
            )
    ));
}

#[test]
fn vertical_single_select_uses_up_down_and_tab_does_not_reset_selection() {
    let mut model = ConfigFormModel::new(select_view());
    model.update(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(model.interaction().selected_option, 1);

    model.update(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.interaction().selected_option, 1);

    model.update(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(model.interaction().selected_option, 0);
}

#[test]
fn text_input_tracks_cursor_for_insertion_and_deletion() {
    let mut view = secret_view();
    view.page.fields[0].field_type = sdk::ConfigFormFieldType::Text;
    view.page.fields[0].display_value = Some("ac".to_string());
    let mut model = ConfigFormModel::new(view);

    model.update(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    model.update(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(model.visible_input(), "abc");
    assert_eq!(model.input_cursor_column(), Some(2));

    model.update(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(model.visible_input(), "ac");
    assert_eq!(model.input_cursor_column(), Some(1));
}

#[test]
fn escape_emits_back_after_initial_page_and_cancel_on_initial_page() {
    let mut child_view = secret_view();
    child_view.page.id = sdk::ConfigFormPageId("edit_endpoint".to_string());
    let mut child_model = ConfigFormModel::new(child_view);
    let child_effect = child_model
        .update(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        child_effect,
        ConfigFormEffect::Back {
            session_id: sdk::ConfigFormSessionId(ref value),
            revision: sdk::ConfigFormRevision(1),
        } if value == "session-1"
    ));

    let mut initial_view = select_view();
    initial_view.page.id = sdk::ConfigFormPageId("select_provider".to_string());
    let mut initial_model = ConfigFormModel::new(initial_view);
    let initial_effect = initial_model
        .update(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(initial_effect, ConfigFormEffect::Cancel { .. }));
}

#[test]
fn action_focus_can_select_non_first_action_and_enter_invokes_it() {
    let mut model = ConfigFormModel::new(action_view());
    model.update(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    assert_eq!(model.interaction().focused_action, 1);
    let effect = model
        .update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        effect,
        ConfigFormEffect::InvokeAction { command }
            if command.action_id.as_str() == "cancel"
    ));
}
#[test]
fn escape_emits_back_with_current_identity_on_child_page() {
    let mut model = ConfigFormModel::new(secret_view());
    let effect = model
        .update(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(
        effect,
        ConfigFormEffect::Back {
            session_id: sdk::ConfigFormSessionId(ref value),
            revision: sdk::ConfigFormRevision(1),
        } if value == "session-1"
    ));
}
