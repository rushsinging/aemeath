use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use tokio::sync::mpsc;

/// 随机烹饪动词，用于"完成"消息。
const DONE_VERBS: [&str; 20] = [
    "Sautéed",
    "Baked",
    "Grilled",
    "Simmered",
    "Roasted",
    "Brewed",
    "Toasted",
    "Stewed",
    "Marinated",
    "Charred",
    "Poached",
    "Steamed",
    "Smoked",
    "Brûléed",
    "Flambéed",
    "Fermented",
    "Pickled",
    "Cured",
    "Seared",
    "Blanched",
];

/// 生成"完成"提示文案，如 `✻ Sautéed for 3s`。
///
/// 末尾保留一个换行：diagnostic 块据此追加一行尾随空行，使完成提示与后续
/// 内容（下一回合用户输入回显等）保持视觉间距（迁移前 push_done 的行为）。
fn done_notice(elapsed: std::time::Duration) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % DONE_VERBS.len();
    let verb = DONE_VERBS.get(idx).copied().unwrap_or(DONE_VERBS[0]);
    let secs = elapsed.as_secs();
    let duration = if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
    format!("✻ {verb} for {duration}\n")
}

impl App {
    pub(super) fn handle_done(
        &mut self,
        _ui_tx: &mpsc::Sender<UiEvent>,
        elapsed: Option<std::time::Duration>,
    ) -> Vec<Effect> {
        if let Some(dur) = elapsed {
            // 完成提示作为系统消息进入 ConversationModel（单一真相源），经 document 渲染。
            self.append_system_notice(done_notice(dur));
        }
        self.spinner_stop();
        self.chat.stop_processing();
        self.model
            .runtime
            .apply(RuntimeIntent::SetStatusNotice(StatusNotice::success(
                "Ready",
            )));
        let mut effects = Vec::new();
        // 自动 reflection 的 spawn 由 executor 执行，此处仅描述 Effect。
        if let Some(effect) = self.maybe_auto_reflect() {
            effects.push(effect);
        }
        // 异步获取 reminders 并推送 recap 行
        if self.agent_client.is_some() {
            effects.push(Effect::FetchReminderRecap);
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::done_notice;
    use std::time::Duration;

    #[test]
    fn test_done_notice_short_duration_uses_seconds() {
        let notice = done_notice(Duration::from_secs(3));
        assert!(notice.starts_with('✻'), "应以 ✻ 开头，实际: {notice}");
        assert!(
            notice.trim_end().ends_with("for 3s"),
            "短耗时应以秒展示，实际: {notice}"
        );
    }

    #[test]
    fn test_done_notice_keeps_trailing_newline_for_spacing() {
        // 末尾换行用于让 diagnostic 块追加尾随空行（间距，迁移回归）。
        let notice = done_notice(Duration::from_secs(1));
        assert!(
            notice.ends_with('\n'),
            "应以换行结尾以提供尾随间距，实际: {notice:?}"
        );
    }

    #[test]
    fn test_done_notice_long_duration_uses_minutes_and_seconds() {
        let notice = done_notice(Duration::from_secs(125));
        assert!(
            notice.trim_end().ends_with("for 2m 5s"),
            "超过 60s 应以分秒展示，实际: {notice}"
        );
    }

    #[test]
    fn test_done_notice_zero_duration() {
        let notice = done_notice(Duration::from_secs(0));
        assert!(
            notice.trim_end().ends_with("for 0s"),
            "边界：0 耗时应展示 0s，实际: {notice}"
        );
    }
}
