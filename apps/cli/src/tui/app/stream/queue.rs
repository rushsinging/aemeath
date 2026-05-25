use crate::tui::app::UiEvent;
use ::runtime::api::core::message::Message;
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

    // --- Bug #49 回归测试 ---
    // Bug #49: last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域。
    // 以下测试覆盖 drain_queued_input 和 append_queued_input 在各出口路径的行为。

    /// drain_queued_input: 有排队消息时返回 Some(messages)
    #[tokio::test]
    async fn test_drain_queued_input_returns_some_when_queue_has_messages() {
        let (tx, mut rx) = mpsc::channel(4);
        let drained = tokio::join!(drain_queued_input(&tx), async {
            match rx.recv().await.unwrap() {
                UiEvent::DrainQueuedInput { reply_tx } => {
                    reply_tx
                        .send(vec!["hello".to_string(), "world".to_string()])
                        .unwrap();
                }
                other => panic!("unexpected event: {other:?}"),
            }
        })
        .0;
        assert_eq!(
            drained,
            Some(vec!["hello".to_string(), "world".to_string()])
        );
    }

    /// drain_queued_input: 队列为空时返回 None
    #[tokio::test]
    async fn test_drain_queued_input_returns_none_when_queue_empty() {
        let (tx, mut rx) = mpsc::channel(4);
        let drained = tokio::join!(drain_queued_input(&tx), async {
            match rx.recv().await.unwrap() {
                UiEvent::DrainQueuedInput { reply_tx } => {
                    reply_tx.send(Vec::new()).unwrap();
                }
                other => panic!("unexpected event: {other:?}"),
            }
        })
        .0;
        assert!(drained.is_none());
    }

    /// drain_queued_input: 通道关闭时返回 None
    #[tokio::test]
    async fn test_drain_queued_input_returns_none_when_channel_closed() {
        let (tx, rx) = mpsc::channel(4);
        drop(rx);
        let drained = drain_queued_input(&tx).await;
        assert!(drained.is_none());
    }

    /// Bug #49 回归：模拟 interrupted 路径 —— 排队消息存在时，
    /// append_queued_input 返回 true 且消息被追加，循环应 continue 而非 break。
    #[tokio::test]
    async fn test_bug49_interrupted_path_drain_preserves_queued_input() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        // Simulate existing messages from prior turns
        let mut messages = vec![Message::user("original")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        // User submitted input during last turn
                        reply_tx
                            .send(vec!["user typed while processing".to_string()])
                            .unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        // Queue was drained → should return true (caller should continue, not break)
        assert!(appended);
        // Original message preserved + queued message appended
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text_content(), "original");
        assert_eq!(messages[1].text_content(), "user typed while processing");
        // MessagesSync sent so UI is updated
        let sync = sync_rx.recv().await.unwrap();
        matches!(sync, UiEvent::MessagesSync(msgs) if msgs.len() == 2);
    }

    /// Bug #49 回归：模拟 stall detector 路径 —— 无排队消息时，
    /// append_queued_input 返回 false，循环应 break。
    #[tokio::test]
    async fn test_bug49_stall_path_no_queue_returns_false() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, _sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("existing")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        // No queued input during stall
                        reply_tx.send(Vec::new()).unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
    }

    /// Bug #49 回归：模拟 API error 路径 —— 排队消息被正确 drain。
    #[tokio::test]
    async fn test_bug49_api_error_path_drain_appends_and_continues() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("partial response")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx
                            .send(vec!["retry input after error".to_string()])
                            .unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(appended);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].text_content(), "retry input after error");
        let _ = sync_rx.recv().await.unwrap(); // consume MessagesSync
    }

    /// Bug #49 回归：模拟 EndTurn/无工具调用路径 —— drain 检查确保用户输入不丢失。
    #[tokio::test]
    async fn test_bug49_end_turn_path_drain_preserves_user_input() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![Message::user("q1"), Message::user("a1")];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx
                            .send(vec!["follow-up question".to_string()])
                            .unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(appended);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].text_content(), "follow-up question");
        let _ = sync_rx.recv().await.unwrap(); // consume MessagesSync
    }

    /// Bug #49 回归：模拟工具轮结果同步后 —— 多条排队消息全部追加。
    #[tokio::test]
    async fn test_bug49_tool_round_drain_appends_multiple_messages() {
        let (queue_tx, mut queue_rx) = mpsc::channel(8);
        let (sync_tx, mut sync_rx) = mpsc::channel(8);
        let mut messages = vec![
            Message::user("do something"),
            Message::user("using tool..."),
        ];

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        // User queued multiple messages during tool execution
                        reply_tx
                            .send(vec![
                                "msg1 during tools".to_string(),
                                "msg2 during tools".to_string(),
                                "msg3 during tools".to_string(),
                            ])
                            .unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(appended);
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[2].text_content(), "msg1 during tools");
        assert_eq!(messages[3].text_content(), "msg2 during tools");
        assert_eq!(messages[4].text_content(), "msg3 during tools");
        let _ = sync_rx.recv().await.unwrap(); // consume MessagesSync
    }

    /// Bug #49 回归：模拟多轮循环连续 drain —— 第一轮 drain 有消息 continue，
    /// 第二轮 drain 无消息 break，验证 append_queued_input 可被多次正确调用。
    #[tokio::test]
    async fn test_bug49_sequential_drain_across_loop_iterations() {
        let (queue_tx, mut queue_rx) = mpsc::channel(8);
        let (sync_tx, mut sync_rx) = mpsc::channel(8);
        let mut messages = vec![Message::user("start")];

        // Iteration 1: queue has input → append and continue
        let first = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx.send(vec!["queued round 1".to_string()]).unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;
        assert!(first);
        assert_eq!(messages.len(), 2);
        let _ = sync_rx.recv().await.unwrap(); // consume MessagesSync from round 1

        // Iteration 2: queue empty → return false (simulates loop break)
        let second = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx.send(Vec::new()).unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;
        assert!(!second);
        // Messages unchanged after empty drain
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].text_content(), "queued round 1");
    }

    /// Bug #49 回归：drain 时原有消息内容不被修改，只有新消息被追加到尾部。
    #[tokio::test]
    async fn test_bug49_drain_never_mutates_existing_messages() {
        let (queue_tx, mut queue_rx) = mpsc::channel(4);
        let (sync_tx, mut sync_rx) = mpsc::channel(4);
        let mut messages = vec![
            Message::user("msg_a"),
            Message::user("msg_b"),
            Message::user("msg_c"),
        ];
        let original_len = messages.len();

        let appended = tokio::join!(
            append_queued_input(&queue_tx, &sync_tx, &mut messages),
            async {
                match queue_rx.recv().await.unwrap() {
                    UiEvent::DrainQueuedInput { reply_tx } => {
                        reply_tx.send(vec!["queued_new".to_string()]).unwrap();
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
        )
        .0;

        assert!(appended);
        assert_eq!(messages.len(), original_len + 1);
        // Original messages untouched
        assert_eq!(messages[0].text_content(), "msg_a");
        assert_eq!(messages[1].text_content(), "msg_b");
        assert_eq!(messages[2].text_content(), "msg_c");
        // New message appended at the end
        assert_eq!(messages[3].text_content(), "queued_new");
        let _ = sync_rx.recv().await.unwrap(); // consume MessagesSync
    }
}
