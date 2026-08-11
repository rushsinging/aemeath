use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::update::intent::AgentIntent;
use tokio::sync::mpsc;

impl App {
    pub(super) fn handle_done(
        &mut self,
        _ui_tx: &mpsc::Sender<UiEvent>,
        _elapsed: Option<std::time::Duration>,
    ) -> Vec<Effect> {
        self.chat.stop_processing();
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::success("Ready"))),
        ));
        let effects = Vec::new();
        // #626：NEVER 在每轮 Done 后自动发 FetchReminderRecap。
        //
        // 该 Effect 会往 runtime 输入通道推 `ChatInputEvent::ListReminders`（executor.rs
        // fetch_reminder_recap_effect，标注为"暂时"实现），而 runtime 的 idle 分支处理
        // 纯查询命令后会掉进 run 执行（#628），导致：Done → 自动 ListReminders → 跑新一轮
        // → Done → 又自动 ListReminders → …… 无限自跑（无用户输入）。
        // recap 生成端（UiEvent::ReminderRecap）本就是 no-op 占位，删除不影响在用功能；
        // 若日后需要 reminder recap，MUST 走不驱动 agent loop 的路径（如 Done 事件携带
        // reminders，或独立查询通道），NEVER 复用会触发 run 的 ListReminders 输入事件。
        effects
    }
}
