use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
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
fn done_notice(elapsed: std::time::Duration) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % DONE_VERBS.len();
    let verb = DONE_VERBS[idx];

    let secs = elapsed.as_secs();
    let duration = if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
    format!("✻ {verb} for {duration}")
}

impl App {
    pub(super) fn handle_done(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        elapsed: Option<std::time::Duration>,
    ) -> Option<Effect> {
        if let Some(dur) = elapsed {
            // 完成提示作为系统消息进入 ConversationModel（单一真相源），经 document 渲染。
            self.append_system_notice(done_notice(dur));
        }
        self.output_area.stop_spinner();
        self.chat.stop_processing();
        self.status_bar.set_success("Ready");
        self.maybe_auto_reflect(ui_tx);
        // 异步获取 reminders 并推送 recap 行
        if self.agent_client.is_some() {
            Some(Effect::FetchReminderRecap)
        } else {
            None
        }
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
            notice.ends_with("for 3s"),
            "短耗时应以秒展示，实际: {notice}"
        );
    }

    #[test]
    fn test_done_notice_long_duration_uses_minutes_and_seconds() {
        let notice = done_notice(Duration::from_secs(125));
        assert!(
            notice.ends_with("for 2m 5s"),
            "超过 60s 应以分秒展示，实际: {notice}"
        );
    }

    #[test]
    fn test_done_notice_zero_duration() {
        let notice = done_notice(Duration::from_secs(0));
        assert!(
            notice.ends_with("for 0s"),
            "边界：0 耗时应展示 0s，实际: {notice}"
        );
    }
}
