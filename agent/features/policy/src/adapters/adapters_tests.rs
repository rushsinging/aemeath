//! Logging-contract tests for [`AllowAllPolicy::evaluate`].
//!
//! Proves the entry / Allow-exit logging contract and verifies that production
//! logs record only `mode`, `capability_count`, and `decision` — never the
//! tool name, workspace path, or run identifiers.

use crate::{AllowAllPolicy, PolicyDecision, PolicyPort, PolicyRequest};
use sdk::ids::{RunId, RunStepId};
use std::sync::Mutex;
use tools::{ToolCapabilities, ToolCapability, ToolName};

/// Captured log lines (level + formatted message) for the policy target.
///
/// 全局共享：`log` facade 只接受一次 logger 注册，多个测试必须共享同一全局缓冲。
/// 因此测试间 **MUST** 通过 `TEST_LOCK` 串行，避免并发 `evaluate` 交叉写入导致
/// entry/exit 顺序错乱或 capability_count 跨测试污染（CI 多核并行下 flaky）。
static CAPTURED: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

/// 测试串行锁：两个测试都写全局 CAPTURED，必须互斥执行。
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Minimal `log::Log` implementation that records every record whose target
/// equals the crate `LOG_TARGET`. This lets the tests observe exactly what the
/// production code emits without depending on a logging backend.
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

fn make_request(tool: &str, caps: ToolCapabilities, workspace: &str) -> PolicyRequest {
    PolicyRequest::new(
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
    // 全局 CAPTURED + 全局 logger 要求两个测试互斥执行。
    let _test_guard = TEST_LOCK.lock().expect("TEST_LOCK poisoned");
    install_capturing_logger();
    CAPTURED.lock().expect("clear capture").clear();

    // Sentinel values that MUST NOT appear in any captured log line.
    let request = make_request(
        "SuperSecretTool",
        ToolCapabilities::single(ToolCapability::ReadWorkspace),
        "/top/secret/workspace",
    );

    assert_eq!(AllowAllPolicy.evaluate(&request), PolicyDecision::Allow);

    let lines = CAPTURED.lock().expect("read capture").clone();
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
    let _test_guard = TEST_LOCK.lock().expect("TEST_LOCK poisoned");
    install_capturing_logger();
    CAPTURED.lock().expect("clear capture").clear();

    let request = make_request(
        "MultiCap",
        ToolCapabilities::from_caps([
            ToolCapability::ReadWorkspace,
            ToolCapability::WriteWorkspace,
        ]),
        "/workspace",
    );

    assert_eq!(AllowAllPolicy.evaluate(&request), PolicyDecision::Allow);

    let joined = CAPTURED
        .lock()
        .expect("read capture")
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        joined.contains("capability_count=2"),
        "two capabilities must produce capability_count=2: {joined:?}",
    );
}
