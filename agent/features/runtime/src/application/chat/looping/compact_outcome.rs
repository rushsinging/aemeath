//! compact 结果应用：冻结旧链 → 替换 messages → 设 summary → 发事件。
//!
//! 从 `loop_runner.rs` 拆出，供主循环和 macro 调用。

use crate::application::chat::looping::compact::CompactOutcome;
use crate::application::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use context::session::ChatChain;
use std::sync::Arc;

/// 应用 compact 结果到 loop 状态：冻结旧链 → 替换 messages → 设 summary → 发 CompactFinished。
pub(crate) async fn apply_compact_outcome<S>(
    sink: &S,
    outcome: CompactOutcome,
    chain: &mut ChatChain,
    frozen_chats: &Arc<std::sync::Mutex<Vec<context::session::ChatSegment>>>,
    active_summary: &mut Option<String>,
    active_summary_arc: &Arc<std::sync::Mutex<Option<String>>>,
) where
    S: ChatEventSink,
{
    // 1. 冻结旧链：把当前活跃段全部冻结
    let old_segments: Vec<context::session::ChatSegment> = chain.active_segments().to_vec();
    if let Ok(mut guard) = frozen_chats.lock() {
        guard.extend(old_segments);
    }

    // 2. 用 compact 结果（summary + recent tail）替换活跃链
    chain.compact(outcome.summary.clone(), outcome.messages);

    // 3. 设 summary
    *active_summary = Some(outcome.summary);
    if let Ok(mut guard) = active_summary_arc.lock() {
        *guard = active_summary.clone();
    }
    sink.send_event(RuntimeStreamEvent::CompactFinished {
        messages: chain.messages_flat(),
    })
    .await;
}
