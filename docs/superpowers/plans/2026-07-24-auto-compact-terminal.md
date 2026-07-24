# Auto-compact Terminal 收口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复自动 compact 返回 `Skipped` 后 Run 被错误中止且 TUI 永久停留在 Thinking 的问题。

**Architecture:** Context 的 typed `Skipped` 在 Runtime 自动 compact adapter 中统一归一为非致命 no-op；shared `RunLauncher` 复用 loop engine 的唯一失败收口，把未处理 engine error 转成 Run 聚合产生的 `Failed` terminal event。Main/Sub caller 继续只消费领域终态，不在 SDK/TUI 补造事件。

**Tech Stack:** Rust 2024、Tokio、Runtime `RunLoopPort`、Context `CompactOutcome`、cargo test/clippy、架构守卫。

---

### Task 1: RunLauncher 未处理错误发布权威失败终态

**Files:**
- Modify: `agent/features/runtime/src/application/run_launcher.rs`
- Create: `agent/features/runtime/src/application/run_launcher_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`

- [ ] **Step 1: 将现有 RunLauncher 测试迁到外置测试文件并写失败回归测试**

在 `run_launcher.rs` 只保留：

```rust
#[cfg(test)]
#[path = "run_launcher_tests.rs"]
mod tests;
```

测试 port 支持在 `drain_input` 返回指定 adapter error，然后断言 launcher 保留 typed
error、发布一个 error 文本保真的 `RunDomainEvent::Failed`，并清理 ActiveRun：

```rust
#[tokio::test]
async fn launch_adapter_error_emits_failed_terminal_and_clears_active_run() {
    let registry = Arc::new(ActiveRunRegistry::default());
    let run_id = RunId::new_v7();
    let mut port = StubPort::failing("compact skipped");

    let result = launch(
        RunLaunchInput {
            run_id: run_id.clone(),
            spec: RunSpec::main(),
            parent_run_id: None,
            cancel: CancellationToken::new(),
        },
        registry.clone(),
        &mut port,
    )
    .await;

    assert!(matches!(
        result,
        RunLaunchResult::Failed(LoopEngineError::Adapter(ref error))
            if error == "compact skipped"
    ));
    let failures = port
        .events_emitted
        .iter()
        .filter_map(|event| match event {
            RunDomainEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(failures, vec!["loop adapter error: compact skipped"]);
    assert!(!registry.claim_terminal(&run_id));
}
```

- [ ] **Step 2: 运行单测并确认 RED**

Run:

```bash
cargo test -p runtime application::run_launcher::tests::launch_adapter_error_emits_failed_terminal_and_clears_active_run -- --exact
```

Expected: FAIL，`failures` 为空，因为当前 launcher 只写日志。

- [ ] **Step 3: 复用 loop engine 的失败收口**

将现有 `fail_run` 调整为 crate 内可复用，并从 `loop_engine.rs` 窄 re-export：

```rust
pub(crate) use engine::fail_run;
```

launcher 在 `run_loop` 返回错误时调用它，terminalization 的二次失败只记录诊断，
返回值仍保留原始 engine error：

```rust
Err(error) => {
    let error_text = error.to_string();
    if let Err(terminal_error) = fail_run(&mut run, port, error_text).await {
        log::error!(
            target: crate::LOG_TARGET,
            "[run_launcher] failed to publish RunFailed: {terminal_error}"
        );
    }
    RunLaunchResult::Failed(error)
}
```

- [ ] **Step 4: 运行 RunLauncher 测试并确认 GREEN**

Run:

```bash
cargo test -p runtime application::run_launcher::tests
```

Expected: 3 tests PASS。

- [ ] **Step 5: 提交 terminal 收口**

```bash
git add agent/features/runtime/src/application/run_launcher.rs \
  agent/features/runtime/src/application/run_launcher_tests.rs \
  agent/features/runtime/src/application/loop_engine.rs \
  agent/features/runtime/src/application/loop_engine/engine.rs
git commit -m "fix(runtime): 发布未处理 Run 错误终态"
```

### Task 2: 自动 compact Skipped 统一为非致命 no-op

**Files:**
- Modify: `agent/features/runtime/src/application/context_coordination.rs`
- Modify: `agent/features/runtime/src/application/context_coordination_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/pre_compact_trigger_tests.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

- [ ] **Step 1: 修改 Main adapter 回归测试并补共享状态测试**

将 `pre_compact_trigger_skips_on_compact_outcome_skipped` 的期望改为：

```rust
assert!(
    result.is_ok(),
    "automatic compact skip must continue the current Run: {result:?}"
);
```

在 `context_coordination_tests.rs` 增加两条测试，证明统一 helper 的状态语义：

```rust
#[test]
fn automatic_compact_committed_resets_usage_and_window() {
    let mut usage = Some(42);
    let mut window = Some("window");
    apply_automatic_compact_outcome(
        &CompactOutcome::Committed(CompactResult {
            summary: "summary".to_string(),
            recent_messages: Vec::new(),
            source_revision: SessionRevision::new(7),
        }),
        &mut usage,
        &mut window,
    );
    assert_eq!(usage, None);
    assert_eq!(window, None);
}

#[test]
fn automatic_compact_skipped_preserves_usage_and_window() {
    let mut usage = Some(42);
    let mut window = Some("window");
    apply_automatic_compact_outcome(
        &CompactOutcome::Skipped(CompactSkipReason::ResumeProtection),
        &mut usage,
        &mut window,
    );
    assert_eq!(usage, Some(42));
    assert_eq!(window, Some("window"));
}
```

- [ ] **Step 2: 运行定向测试并确认 RED**

Run:

```bash
cargo test -p runtime pre_compact_trigger_skips_on_compact_outcome_skipped -- --exact
```

Expected: FAIL，当前 Main adapter 把 `Skipped` 返回为 error。

- [ ] **Step 3: 实现单一状态归一 helper 并接入 Main/Sub**

在 `context_coordination.rs` 定义一次：

```rust
pub(crate) fn apply_automatic_compact_outcome<T>(
    outcome: &CompactOutcome,
    last_total_tokens: &mut Option<u64>,
    context_window: &mut Option<T>,
) {
    if matches!(outcome, CompactOutcome::Committed(_)) {
        *last_total_tokens = None;
        *context_window = None;
    }
}
```

Main 在 PreCompact reflection 判定后调用 helper 并返回 `Ok(())`；Sub 对 compact
outcome 调同一个 helper 并返回 `Ok(())`。`Skipped` 不清 usage/window，
`Committed` 保持原有 reset 行为。

- [ ] **Step 4: 运行 Main、共享 helper 和 Sub Runtime 测试并确认 GREEN**

Run:

```bash
cargo test -p runtime pre_compact_trigger
cargo test -p runtime application::context_coordination::tests
cargo test -p runtime application::subagent::runner
```

Expected: 全部 PASS；`Skipped` 不提交 PreCompact reflection。

- [ ] **Step 5: 清理 #1380 已退役的 Sub registration 路径**

从 `loop_run.rs` 删除未使用的 `shared_run_loop` / `Run` imports、
`ActiveRunRegistration` 及只验证该死代码的测试。Run 生命周期唯一由
`RunLauncher` 管理。

- [ ] **Step 6: 提交 compact 语义修复**

```bash
git add agent/features/runtime/src/application/context_coordination.rs \
  agent/features/runtime/src/application/context_coordination_tests.rs \
  agent/features/runtime/src/application/main_loop/looping/main_run_port.rs \
  agent/features/runtime/src/application/main_loop/looping/pre_compact_trigger_tests.rs \
  agent/features/runtime/src/application/subagent/runner/loop_run.rs
git commit -m "fix(runtime): compact 跳过后继续 Run"
```

### Task 3: 文档门禁、完整验证与推送

**Files:**
- Modify: `docs/superpowers/specs/2026-07-24-auto-compact-terminal-design.md` only if implementation changes the approved contract
- Modify: GitHub Issue `#1387`
- Modify: Release Gate Issue `#579`

- [ ] **Step 1: 格式与差异检查**

Run:

```bash
cargo fmt --all --check
git diff --check
git status --short
```

Expected: exit 0；仅出现计划内文件。

- [ ] **Step 2: Runtime 与契约测试**

Run:

```bash
cargo test -p runtime
```

Expected: 所有 Runtime unit/integration/doc tests PASS。

- [ ] **Step 3: Workspace 编译与 lint**

Run:

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: exit 0，零 warning。

- [ ] **Step 4: 架构守卫**

Run:

```bash
bash .agents/hooks/check-architecture-guards.sh
```

Expected: 所有架构守卫 PASS。

- [ ] **Step 5: 同步最新 main 并推送**

Run:

```bash
git pull --no-rebase origin main
git push -u origin fix/1387-auto-compact-terminal
```

Expected: push 成功，pre-push hook 完整通过。

- [ ] **Step 6: 更新 Issue 与 Release Gate**

在 #1387 勾选已完成的文档、L0/L2/L3 checklist，评论提交 SHA、验证命令和分支；
在 #579 回写阻断已修复、等待 PR/review。Issue 保持 open，等待用户确认或 PR 合入。
