use super::{FieldPatch, LogContext, LogContextPatch};

fn parent_context() -> LogContext {
    LogContext {
        session_id: Some("session-parent".to_string()),
        chat_id: Some("chat-parent".to_string()),
        turn: Some(7),
        request_id: Some("request-parent".to_string()),
        model: Some("model-parent".to_string()),
        provider: Some("provider-parent".to_string()),
        role: Some("role-parent".to_string()),
    }
}

#[test]
fn patch_applies_inherit_set_and_clear_without_mutating_parent() {
    let parent = parent_context();
    let patch = LogContextPatch {
        session_id: FieldPatch::Inherit,
        chat_id: FieldPatch::Set("chat-child".to_string()),
        turn: FieldPatch::Set(0),
        request_id: FieldPatch::Clear,
        model: FieldPatch::Inherit,
        provider: FieldPatch::Clear,
        role: FieldPatch::Set("role-child".to_string()),
    };

    let child = parent.patched(patch);

    assert_eq!(child.session_id.as_deref(), Some("session-parent"));
    assert_eq!(child.chat_id.as_deref(), Some("chat-child"));
    assert_eq!(child.turn, Some(0));
    assert_eq!(child.request_id, None);
    assert_eq!(child.model.as_deref(), Some("model-parent"));
    assert_eq!(child.provider, None);
    assert_eq!(child.role.as_deref(), Some("role-child"));
    assert_eq!(parent.turn, Some(7));
    assert_eq!(parent.request_id.as_deref(), Some("request-parent"));
}

#[test]
fn default_patch_inherits_every_field() {
    let parent = parent_context();

    assert_eq!(parent.patched(LogContextPatch::default()), parent);
}
