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
