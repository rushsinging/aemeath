use crate::tui::core::event::UiEvent;
use crate::tui::core::App;

impl App {
    pub(super) async fn handle_memory_command(
        &mut self,
        ui_tx: Option<tokio::sync::mpsc::Sender<UiEvent>>,
    ) {
        if let Some(ref ac) = self.agent_client {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    if let Some(tx) = &ui_tx {
                        let _ = tx.send(UiEvent::MemoryList(reminders)).await;
                    }
                }
                Err(e) => {
                    self.output_area
                        .push_error(&format!("获取 reminders 失败: {e}"));
                }
            }
        }
    }
}
