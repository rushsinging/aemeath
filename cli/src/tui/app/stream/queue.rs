use crate::tui::app::UiEvent;
use aemeath_core::message::Message;
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

pub(crate) async fn append_queued_input(
    queue_request_tx: &mpsc::Sender<UiEvent>,
    sync_tx: &mpsc::Sender<UiEvent>,
    messages: &mut Vec<Message>,
) -> bool {
    let Some(queued) = drain_queued_input(queue_request_tx).await else {
        return false;
    };
    for input in queued {
        messages.push(Message::user(input));
    }
    let _ = sync_tx.send(UiEvent::MessagesSync(messages.clone())).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_queued_input_happy_path_appends_and_syncs() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("first")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx
                            .send(vec!["queued one".to_string(), "queued two".to_string()])
                            .unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(appended);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].text_content(), "queued one");
        assert_eq!(messages[2].text_content(), "queued two");
        match sync_rx.recv().await.unwrap() {
            UiEvent::MessagesSync(sync_messages) => assert_eq!(sync_messages.len(), 3),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_append_queued_input_boundary_empty_queue_returns_false() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("first")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => reply_tx.send(Vec::new()).unwrap(),
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sync_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_append_queued_input_error_closed_queue_returns_false() {
        let (queue_tx, queue_rx) = mpsc::channel(4);
        drop(queue_rx);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue_tx, &sync_tx, &mut messages).await;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sync_rx.try_recv().is_err());
    }
}
