//! Logging-contract tests for [`allow_all()::evaluate`].
//!
//! Proves the entry / Allow-exit logging contract and verifies that production
//! logs record only `mode`, `capability_count`, and `decision` — never the
//! tool name, workspace path, or run identifiers.

use crate::{allow_all, PolicyDecisionData, PolicyRequestData};
use share::ids::{RunId, RunStepId};
use share::tools_vocab::{ToolCapabilities, ToolCapability, ToolName};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::thread::ThreadId;

/// Captured log lines (level + formatted message) for the policy target,
/// bucketed per emitting thread.
///
/// `log` facade 只接受一次 logger 注册，缓冲必然进程级共享；因此按
/// **线程隔离**：每条记录写入发射线程自己的桶，测试只读取/断言本线程桶。
/// 不持锁的并行测试（如 contract 测试调 `evaluate`）的日志写入各自桶，
/// NEVER 串扰本测试的捕获窗口（跨线程串扰曾是 pre-push 环境
/// `entry must precede exit` flake 的根因）。
static CAPTURED: LazyLock<Mutex<HashMap<ThreadId, Vec<(log::Level, String)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Drain this thread's capture bucket (take all records, leaving it empty).
fn drain_thread_capture() -> Vec<(log::Level, String)> {
    CAPTURED
        .lock()
        .expect("CAPTURED lock poisoned")
        .remove(&std::thread::current().id())
        .unwrap_or_default()
}

/// Minimal `log::Log` implementation that records every record whose target
/// equals the crate `LOG_TARGET`, into the emitting thread's bucket. This lets
/// the tests observe exactly what the production code emits without depending
/// on a logging backend.
struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target() == crate::LOG_TARGET
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            CAPTURED
                .lock()
                .expect("CAPTURED lock poisoned")
                .entry(std::thread::current().id())
                .or_default()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

/// Install the capturing logger once for the whole process. Idempotent — the
/// `log` facade only accepts the first registration; subsequent calls are
/// harmless because the same global buffer is reused.
fn install_capturing_logger() {
    let _ = log::set_boxed_logger(Box::new(CapturingLogger));
    log::set_max_level(log::LevelFilter::Trace);
}

fn make_request(tool: &str, caps: ToolCapabilities, workspace: &str) -> PolicyRequestData {
    PolicyRequestData::new(
        RunId::new_v7(),
        RunStepId::new_v7(),
        ToolName::new(tool),
        caps,
        workspace,
    )
    .expect("valid request")
}

/// Contract: `evaluate` emits an **entry** log line recording the policy
/// `mode` and the `capability_count`, then an **exit** log line recording the
/// `decision` (`Allow`). Both lines are `debug` level. No captured line may
/// contain the tool name or the workspace path.
#[test]
fn evaluate_emits_entry_and_allow_exit_logging_only_mode_count_and_decision() {
    install_capturing_logger();
    let _ = drain_thread_capture();

    // Sentinel values that MUST NOT appear in any captured log line.
    let request = make_request(
        "SuperSecretTool",
        ToolCapabilities::single(ToolCapability::Read),
        "/top/secret/workspace",
    );

    assert_eq!(
        allow_all().evaluate(&request),
        PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::ALLOW_ALL)
    );

    let lines = drain_thread_capture();
    assert!(
        lines.len() >= 2,
        "expected at least entry + exit log lines, got {lines:?}",
    );

    // All production policy logs are debug level (per-turn diagnostic detail).
    assert!(
        lines.iter().all(|(lvl, _)| *lvl == log::Level::Debug),
        "policy logs must be debug level, got {lines:?}",
    );

    let joined = lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // Entry log: mode + capability count.
    assert!(joined.contains("entry"), "entry log missing: {joined:?}");
    assert!(
        joined.contains("mode=AllowAll"),
        "policy mode must be logged: {joined:?}",
    );
    assert!(
        joined.contains("capability_count=1"),
        "capability count (1) must be logged: {joined:?}",
    );

    // Exit log: decision.
    assert!(
        joined.contains("exit") && joined.contains("decision=Allow"),
        "exit log with decision=Allow missing: {joined:?}",
    );

    // Entry must precede exit (ordering contract).
    let entry_idx = joined.find("entry").expect("entry marker present");
    let exit_idx = joined.find("exit").expect("exit marker present");
    assert!(
        entry_idx < exit_idx,
        "entry log must precede exit log: {joined:?}",
    );

    // Privacy contract: neither tool name nor workspace path may leak.
    for (_, msg) in &lines {
        assert!(
            !msg.contains("SuperSecretTool"),
            "tool name leaked into production log: {msg:?}",
        );
        assert!(
            !msg.contains("secret"),
            "workspace path leaked into production log: {msg:?}",
        );
    }
}

/// Capability count reflects the number of required capabilities, not a fixed
/// constant — a request with two capabilities must log `capability_count=2`.
#[test]
fn capability_count_reflects_request_requirements() {
    install_capturing_logger();
    let _ = drain_thread_capture();

    let request = make_request(
        "MultiCap",
        ToolCapabilities::from_caps([ToolCapability::Read, ToolCapability::Write]),
        "/workspace",
    );

    assert_eq!(
        allow_all().evaluate(&request),
        PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::ALLOW_ALL)
    );

    let joined = drain_thread_capture()
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        joined.contains("capability_count=2"),
        "two capabilities must produce capability_count=2: {joined:?}",
    );
}

/// Deterministic cross-thread interference repro.
///
/// 并行测试（如 contract 测试调 `evaluate`，不持 `TEST_LOCK`）产生的同 target
/// 日志 MUST NOT 泄入本测试的捕获窗口。全局缓冲实现下本测试失败（跨线程串扰，
/// 即 pre-push 环境 `entry must precede exit` flake 的根因）；线程隔离实现下通过。
#[test]
fn captured_records_are_isolated_from_other_threads() {
    install_capturing_logger();
    let _ = drain_thread_capture();

    std::thread::spawn(|| {
        log::debug!(target: crate::LOG_TARGET, "policy evaluate exit: interference");
    })
    .join()
    .expect("interference thread join");

    let request = make_request(
        "IsoThreadTool",
        ToolCapabilities::single(ToolCapability::Read),
        "/iso/workspace",
    );
    let _ = allow_all().evaluate(&request);

    let lines = drain_thread_capture();
    assert!(
        lines.iter().all(|(_, msg)| !msg.contains("interference")),
        "cross-thread log leaked into this test's capture window: {lines:?}",
    );
}
