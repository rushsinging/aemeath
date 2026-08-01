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

#[test]
fn escape_emits_cancel_with_current_identity() {
    let mut model = ConfigFormModel::new(secret_view());
    let effect = model
        .update(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(
        effect,
        ConfigFormEffect::Cancel {
            session_id: sdk::ConfigFormSessionId(ref value),
            revision: sdk::ConfigFormRevision(1),
        } if value == "session-1"
    ));
}
