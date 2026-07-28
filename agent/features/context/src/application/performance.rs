use std::cell::RefCell;
use std::future::Future;
use std::time::Duration;

use crate::domain::DecisionReason;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ContextBuildPerformanceSnapshot {
    pub build_calls: u64,
    pub build_ns: u64,
    pub snapshot_calls: u64,
    pub snapshot_ns: u64,
    pub prompt_calls: u64,
    pub prompt_ns: u64,
    pub memory_calls: u64,
    pub memory_ns: u64,
    pub assembly_calls: u64,
    pub assembly_ns: u64,
    pub decision_calls: u64,
    pub decision_ns: u64,
    pub backing_revision: u64,
    pub snapshot_messages: usize,
    pub snapshot_committed_steps: usize,
    pub snapshot_shared_messages: usize,
    pub pending_messages: usize,
    pub final_messages: usize,
    pub system_blocks: usize,
    pub tool_result_blocks: usize,
    pub tool_result_content_bytes: u64,
    pub estimated_total_tokens: usize,
    pub provider_actual_tokens: Option<u64>,
    pub decision_token_count: usize,
    pub decision_reason: Option<DecisionReason>,
}

thread_local! {
    static ACTIVE_CAPTURE: RefCell<Option<ContextBuildPerformanceSnapshot>> = const { RefCell::new(None) };
}

struct CaptureGuard {
    active: bool,
}

impl CaptureGuard {
    fn start() -> Self {
        ACTIVE_CAPTURE.with(|capture| {
            assert!(
                capture.borrow().is_none(),
                "context performance capture 不支持嵌套"
            );
            *capture.borrow_mut() = Some(ContextBuildPerformanceSnapshot::default());
        });
        Self { active: true }
    }

    fn finish(mut self) -> ContextBuildPerformanceSnapshot {
        self.active = false;
        ACTIVE_CAPTURE.with(|capture| {
            capture
                .borrow_mut()
                .take()
                .expect("context performance capture scope 应保持有效")
        })
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_CAPTURE.with(|capture| {
                capture.borrow_mut().take();
            });
        }
    }
}

pub(crate) async fn capture<T>(
    run: impl Future<Output = T>,
) -> (T, ContextBuildPerformanceSnapshot) {
    let guard = CaptureGuard::start();
    let value = run.await;
    (value, guard.finish())
}

fn update(update: impl FnOnce(&mut ContextBuildPerformanceSnapshot)) {
    ACTIVE_CAPTURE.with(|capture| {
        if let Some(snapshot) = capture.borrow_mut().as_mut() {
            update(snapshot);
        }
    });
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

pub(crate) fn record_build(duration: Duration) {
    update(|snapshot| {
        snapshot.build_calls += 1;
        snapshot.build_ns = snapshot.build_ns.saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_snapshot(
    revision: u64,
    snapshot_messages: usize,
    snapshot_committed_steps: usize,
    snapshot_shared_messages: usize,
    duration: Duration,
) {
    update(|snapshot| {
        snapshot.snapshot_calls += 1;
        snapshot.snapshot_ns = snapshot.snapshot_ns.saturating_add(duration_ns(duration));
        snapshot.backing_revision = revision;
        snapshot.snapshot_messages = snapshot_messages;
        snapshot.snapshot_committed_steps = snapshot_committed_steps;
        snapshot.snapshot_shared_messages = snapshot_shared_messages;
    });
}

pub(crate) fn record_prompt(duration: Duration) {
    update(|snapshot| {
        snapshot.prompt_calls += 1;
        snapshot.prompt_ns = snapshot.prompt_ns.saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_memory(duration: Duration) {
    update(|snapshot| {
        snapshot.memory_calls += 1;
        snapshot.memory_ns = snapshot.memory_ns.saturating_add(duration_ns(duration));
    });
}

pub(crate) struct AssemblyMetrics {
    pub pending_messages: usize,
    pub final_messages: usize,
    pub system_blocks: usize,
    pub tool_result_blocks: usize,
    pub tool_result_content_bytes: u64,
}

pub(crate) fn record_assembly(metrics: AssemblyMetrics, duration: Duration) {
    update(|snapshot| {
        snapshot.assembly_calls += 1;
        snapshot.assembly_ns = snapshot.assembly_ns.saturating_add(duration_ns(duration));
        snapshot.pending_messages = metrics.pending_messages;
        snapshot.final_messages = metrics.final_messages;
        snapshot.system_blocks = metrics.system_blocks;
        snapshot.tool_result_blocks = metrics.tool_result_blocks;
        snapshot.tool_result_content_bytes = metrics.tool_result_content_bytes;
    });
}

pub(crate) fn record_decision(
    estimated_total_tokens: usize,
    provider_actual_tokens: Option<u64>,
    decision_token_count: usize,
    decision_reason: DecisionReason,
    duration: Duration,
) {
    update(|snapshot| {
        snapshot.decision_calls += 1;
        snapshot.decision_ns = snapshot.decision_ns.saturating_add(duration_ns(duration));
        snapshot.estimated_total_tokens = estimated_total_tokens;
        snapshot.provider_actual_tokens = provider_actual_tokens;
        snapshot.decision_token_count = decision_token_count;
        snapshot.decision_reason = Some(decision_reason);
    });
}

pub(crate) fn percentiles_ns(samples: &[u64]) -> Option<(u64, u64)> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Some((nearest_rank(&sorted, 50), nearest_rank(&sorted, 95)))
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100).max(1);
    sorted[rank - 1]
}
