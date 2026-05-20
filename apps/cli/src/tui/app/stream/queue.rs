use crate::tui::app::UiEvent;
use tokio::sync::mpsc;

pub(crate) async fn drain_queued_input(tx: &mpsc::Sender<UiEvent>) -> Option<Vec<String>> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if tx
        .send(UiEvent::DrainQueuedInput { reply_tx })
        .await
        .is_err()
    {
        return None;
    }
    match reply_rx.await {
        Ok(queued) if !queued.is_empty() => Some(queued),
        _ => None,
    }
}
