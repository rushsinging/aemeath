//! 小型辅助函数，从 `loop_runner` 中提取以减小文件体积。

use crate::business::chat::looping::{
    apply_gate, drain_sources, ChatEventSink, GateKind, InputEventDrainPort, PendingInputBuffer,
    QueueDrainPort,
};
use crate::business::chat::GateOutcome;
use crate::business::session::ChatChain;

/// 判断 provider 错误是否为用户主动取消。
pub(crate) fn is_user_cancelled_provider_error(error: &provider::api::LlmError) -> bool {
    error.is_cancelled()
}

/// 排空输入队列并应用 gate 决策。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drain_and_apply_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queue: &Q,
    input_events: &I,
    sink: &S,
    chain: &mut ChatChain,
    segment_id: &str,
    task_store: &storage::api::TaskStore,
) -> GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    drain_sources(buffer, queue, input_events).await;
    apply_gate(kind, buffer, sink, chain, segment_id, task_store, false).await
}
