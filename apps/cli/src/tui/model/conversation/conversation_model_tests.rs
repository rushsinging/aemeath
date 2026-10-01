use super::super::intent::*;
use super::*;

#[test]
fn test_revision_starts_at_zero() {
    let model = ConversationModel::default();
    assert_eq!(model.revision(), 0, "新建 conversation revision 应为 0");
}

#[test]
fn test_revision_bumps_on_mutating_apply() {
    let mut model = ConversationModel::default();
    let before = model.revision();
    let changes = model.apply(AppendUserMessage {
        text: "你好".to_string(),
    });
    assert!(!changes.is_empty(), "AppendUserMessage 应产生 change");
    assert_eq!(
        model.revision(),
        before + 1,
        "产生 change 的 apply 应使 revision +1"
    );
}

#[test]
fn test_revision_unchanged_on_noop_apply() {
    let mut model = ConversationModel::default();
    let before = model.revision();
    // 空文本的 AssistantText 返回空 change（no-op）。
    let changes = model.apply(AssistantText {
        chat_id: ChatId::new("c1"),
        run_id: ChatRunId::new("t1"),
        text: String::new(),
    });
    assert!(changes.is_empty(), "空文本 AssistantText 应为 no-op");
    assert_eq!(model.revision(), before, "no-op apply 不应改 revision");
}
