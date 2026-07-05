//! 小型辅助函数，从 `loop_runner` 中提取以减小文件体积。

use crate::business::chat::looping::{
    apply_gate, drain_sources, ChatEventSink, GateKind, InputEventDrainPort, PendingInputBuffer,
    QueueDrainPort,
};
use crate::business::chat::GateOutcome;
use share::message::Message;

/// 判断 provider 错误是否为用户主动取消。
pub(crate) fn is_user_cancelled_provider_error(error: &provider::api::LlmError) -> bool {
    error.is_cancelled()
}

/// 排空输入队列并应用 gate 决策。
///
/// **queued_buffer drain（#632）**：busy select! 期间用户消息被暂存到
/// `queued_buffer`（本地 `VecDeque<ChatInputEvent>`），而非 `pending_input`。
/// 此函数在 `drain_sources` 之前先把 `queued_buffer` 全部转移到 `buffer`，
/// 确保每个 gate 点（BeforeLlm / AfterBlockingBoundary / BeforeFinish）
/// 都能正确看到排队消息，不会遗漏。
pub(crate) async fn drain_and_apply_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queued_buffer: &mut std::collections::VecDeque<sdk::ChatInputEvent>,
    queue: &Q,
    input_events: &I,
    sink: &S,
    messages: &mut Vec<Message>,
    task_store: &storage::api::TaskStore,
) -> GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    // #632: 先 drain queued_buffer（busy select! 期间排队的输入）。
    while let Some(event) = queued_buffer.pop_front() {
        buffer.push(event);
    }
    drain_sources(buffer, queue, input_events).await;
    apply_gate(kind, buffer, sink, messages, task_store, false).await
}
