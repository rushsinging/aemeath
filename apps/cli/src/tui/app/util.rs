use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::update::intent::AgentIntent;
use std::time::{Duration, Instant};

/// 临时 status notice 存活时长。
const TRANSIENT_NOTICE_TTL: Duration = Duration::from_secs(5);

impl super::App {
    /// 设置临时 status notice，`TRANSIENT_NOTICE_TTL` 后由 SpinnerTick 自动回退到
    /// graph_phase 派生的持久态。
    pub(crate) fn set_transient_notice(&mut self, notice: StatusNotice) {
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::SetTransientStatusNotice(SetTransientStatusNotice {
                notice,
                expires_at: Instant::now() + TRANSIENT_NOTICE_TTL,
            }),
        ));
    }
    /// 描述「复制文本到剪贴板」副作用，返回 CopyToClipboard Effect（不在此处做 IO）。
    /// 实际的剪贴板写入与 status bar 反馈由 effect/executor 执行。
    pub fn copy_to_clipboard(&self, text: &str) -> Effect {
        Effect::CopyToClipboard {
            text: text.to_string(),
        }
    }

    /// 描述「复制可选选区文本」副作用；None 时返回 None（不复制）。
    pub fn copy_selection_to_clipboard(&self, text: Option<String>) -> Option<Effect> {
        text.map(|t| self.copy_to_clipboard(&t))
    }

    /// Accept the currently highlighted suggestion
    pub fn apply_current_suggestion(&mut self) {
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::AcceptCompletion);
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use crate::tui::effect::effect::Effect;
    use std::path::PathBuf;

    fn make_app() -> App {
        App::new("s".to_string(), PathBuf::from("/tmp"), "m".to_string())
    }

    #[test]
    fn test_copy_to_clipboard_returns_effect() {
        let app = make_app();
        let effect = app.copy_to_clipboard("hello");
        assert!(matches!(effect, Effect::CopyToClipboard { text } if text == "hello"));
    }

    #[test]
    fn test_copy_selection_to_clipboard_some_returns_effect() {
        let app = make_app();
        let effect = app.copy_selection_to_clipboard(Some("sel".to_string()));
        assert!(matches!(
            effect,
            Some(Effect::CopyToClipboard { text }) if text == "sel"
        ));
    }

    #[test]
    fn test_copy_selection_to_clipboard_none_returns_none() {
        let app = make_app();
        assert!(app.copy_selection_to_clipboard(None).is_none());
    }

    #[test]
    fn test_apply_current_suggestion_accepts_model_completion() {
        let mut app = make_app();
        app.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertText(
            "/he now".to_string(),
        ));
        app.handle_input_intent(crate::tui::model::input::intent::InputIntent::MoveCursor(3));
        app.handle_input_intent(
            crate::tui::model::input::intent::InputIntent::SetCompletions {
                query: "/he now".to_string(),
                items: vec![
                    crate::tui::model::input::completion_item::CompletionItem::new(
                        "/help", "/help",
                    ),
                ],
            },
        );

        app.apply_current_suggestion();

        assert_eq!(app.model.input.document.buffer, "/help now");
    }
}
