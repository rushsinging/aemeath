# #1931 BackgroundProcessWatch 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增强制前台、带超时的 `BackgroundProcessWatch`，给 LLM 主动短等后台任务的出口；含 SDK/TUI/L0–L5。

**Architecture:** 沿用 #252 工具族。`BackgroundProcessAccess` 增加 async `wait_task`（runtime 内轮询账本 + timeout + cancel）；tool adapter 编排 clamp/outcome；`execution_supervisor` 对该工具名强制 `background_threshold=None`；完成通知/reminder 不改；fuse 不豁免。TUI 按 #1895 四工具模式加第五个 Display。

**Tech Stack:** Rust / tokio / async_trait / serde / inventory ToolDisplay / sdk re-export

---

## 拍板摘要（勿改）

| 项 | 值 |
|---|---|
| timeout 缺省 | 60s |
| 硬上限 | 300s（clamp 到 [1, 300]） |
| 转后台 | 禁止 |
| 完成通知 | 不吞 |
| Fuse | 不豁免 |
| Caps | TaskRead |

---

### Task 1: types + timeout clamp + serde 单测

**Files:**
- Modify: `agent/features/tools/src/domain/types/background_processes.rs`
- Modify: `agent/features/tools/src/domain/types.rs`（re-export）
- Create/Modify: 同目录或 `background_processes` 旁测试（若已有 `*_tests.rs` 则追加，否则 `#[cfg(test)]` 同文件）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn watch_timeout_defaults_to_60_and_clamps_to_1_300() {
    assert_eq!(resolve_watch_timeout_secs(None), 60);
    assert_eq!(resolve_watch_timeout_secs(Some(0)), 1);
    assert_eq!(resolve_watch_timeout_secs(Some(301)), 300);
    assert_eq!(resolve_watch_timeout_secs(Some(120)), 120);
}

#[test]
fn watch_result_serde_round_trip() {
    let result = BackgroundProcessWatchResult {
        outcome: BackgroundProcessWatchOutcome::StillRunning,
        detail: /* minimal DetailData */,
    };
    let json = serde_json::to_value(&result).unwrap();
    let back: BackgroundProcessWatchResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.outcome, BackgroundProcessWatchOutcome::StillRunning);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p tools --lib watch_timeout_defaults_to_60 -- --nocapture`
Expected: FAIL（符号不存在）

- [ ] **Step 3: 最小实现**

新增：
- `BackgroundProcessWatchInput { task_id: String, timeout_secs: Option<u64> }`
- `BackgroundProcessWatchOutcome { Terminal, StillRunning, Cancelled }`（serde rename_all = "snake_case"）
- `BackgroundProcessWatchResult { outcome, detail: BackgroundProcessDetailData }`
- `pub fn resolve_watch_timeout_secs(raw: Option<u64>) -> u64`：None→60；Some(v)→v.clamp(1,300)
- 常量 `DEFAULT_WATCH_TIMEOUT_SECS=60`、`MAX_WATCH_TIMEOUT_SECS=300` 放 `tools` 侧合适 constants（或同文件；若触发 constant-placement 守卫则归 `constants.rs`）

- [ ] **Step 4: 跑测试至 GREEN**

Run: `cargo test -p tools --lib watch_timeout_defaults_to_60 watch_result_serde_round_trip`
（若 cargo 只接受一个 filter：分两次跑）

- [ ] **Step 5: Commit**

```bash
git add agent/features/tools/src/domain/types/background_processes.rs agent/features/tools/src/domain/types.rs
git commit -m "$(cat <<'EOF'
feat(tools): #1931 BackgroundProcessWatch 类型与 timeout clamp

EOF
)"
```

---

### Task 2: port `wait_task` + runtime 实现 + L2 测

**Files:**
- Modify: `agent/features/tools/src/domain/background_process_port.rs`
- Modify: `agent/features/runtime/src/application/background_process/session_runtime.rs`
- Modify: `agent/features/runtime/src/application/background_process/supervisor.rs`（若需 Notify/轮询辅助）
- Modify: tools/runtime 内所有 `BackgroundProcessAccess` stub（`registry_tests.rs` 等）
- Test: `session_runtime` 或新建 `wait_task` 测；supervisor 级亦可

- [ ] **Step 1: 写失败测试（runtime）**

覆盖：
1. 已终态 → 立即 `Terminal`
2. 运行中等到超时 → `StillRunning`
3. 未知 id → Err
4. cancel 信号 → `Cancelled`（或 Err，与工具层约定一致；推荐 outcome=Cancelled + 最后已知 detail，无 detail 则 error）

- [ ] **Step 2: 扩展 port**

```rust
#[async_trait]
pub trait BackgroundProcessAccess: Send + Sync {
    // 既有 sync 方法保持
    async fn wait_task(
        &self,
        task_id: &str,
        timeout: std::time::Duration,
        cancellation: std::sync::Arc<dyn crate::domain::context::CancellationSignal>,
    ) -> Result<crate::domain::types::background_processes::BackgroundProcessWatchResult, String>;
}
```

注释改为：查询方法不阻塞；`wait_task` 是 Watch 专用可取消等待。

- [ ] **Step 3: runtime 实现**

`BackgroundProcessRuntime::wait_task`：
- parse id；无记录 → Err("Task not found: ...")
- 轮询间隔建议 100ms（常量归位）
- `task_status` 状态 ∈ `{succeeded,failed,timed_out,stopped,invalidated}` → Terminal
- `foreground_waiting` / `backgrounded` → 继续等
- 超时 → StillRunning + 当前 detail
- `cancellation.is_cancelled()` → Cancelled
- **NEVER** 修改完成通知 / notify_terminal 路径

- [ ] **Step 4: 更新所有 stub impl**（编译通过）

- [ ] **Step 5: 测试 GREEN + Commit**

```bash
cargo test -p runtime --lib wait_task
cargo test -p tools --lib
git commit -m "$(cat <<'EOF'
feat(runtime): #1931 BackgroundProcessAccess::wait_task

EOF
)"
```

---

### Task 3: tool adapter + registry + 禁转后台 + i18n/prompt

**Files:**
- Modify: `agent/features/tools/src/adapters/background_processes.rs`
- Modify: `agent/features/tools/src/adapters/registry.rs`（+ registry_tests FULL 列表若有）
- Modify: `agent/features/runtime/src/application/tool/execution_supervisor.rs`
- Modify: `agent/shared/src/i18n/tools/background.rs`（+ i18n 导出）
- Modify: `agent/features/runtime/src/application/prompt/prompt_build_ext.rs`
- Test: tools adapter 测；runtime「Watch 不进 backgrounding」测

- [ ] **Step 1: 失败测试**
  - Watch tool：终态/超时/未知 id（mock access）
  - execution_supervisor：工具名 `BackgroundProcessWatch` 时即使 threshold=Some(10s) 也不 `move_to_background`（仿现有 background 测）
  - fuse：确认 `TOOL_FUSE_EXEMPT_READONLY_BACKGROUND_TOOLS` **不含** Watch（断言数组）

- [ ] **Step 2: 实现 `BackgroundProcessWatchTool`**
  - `resolve_watch_timeout_secs`
  - `source.current().wait_task(...).await`
  - 成功文本含 outcome + state

- [ ] **Step 3: registry 注册 `Caps::TaskRead`**

- [ ] **Step 4: 禁转后台**
  在 `execute` 计算 `backgrounding` 前：若 `call.identity.tool_name == "BackgroundProcessWatch"` 则视为 `None`。

- [ ] **Step 5: i18n + prompt**
  - zh/en：说明前台短等、勿连续狂 Watch、优先完成通知、不豁免 fuse
  - `prompt_build_ext` 后台段落增加 Watch 指引一行

- [ ] **Step 6: GREEN + Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tools): #1931 注册 BackgroundProcessWatch 并禁止转后台

EOF
)"
```

---

### Task 4: SDK 导出 + TUI header/result

**Files:**
- Modify: `packages/sdk/src/tool_input.rs` / `tool_result.rs`（及 mod 导出）
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/background_process.rs`
- Modify: `apps/cli/src/tui/view_model/state.rs`（TOOL_DISPLAY_NAMES）
- Modify: `background_process_tests.rs` / `tool_result_tests.rs`

- [ ] **Step 1: 失败测试**
  - `format_header` 含 task_id（及 timeout 可选）
  - `format_result_lines` 解析 WatchResult，渲染 outcome + state（非 JSON 原文）
  - `test_lookup_display_finds_background_process_family` 含 Watch
  - `render_tool_result` 管线测（仿 List/Logs）

- [ ] **Step 2: SDK re-export Input/Result/Outcome**

- [ ] **Step 3: `BackgroundProcessWatchDisplay`**
  - header: `Backend Process Watch` + task_id
  - result: `outcome · state · duration` 风格对齐 Status
  - `TOOL_DISPLAY_NAMES`: `("BackgroundProcessWatch", "Backend Process Watch")`

- [ ] **Step 4: GREEN + Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(cli): #1931 BackgroundProcessWatch TUI/SDK 显示与解析

EOF
)"
```

---

### Task 5: 设计文档 + L5/门禁收口

**Files:**
- Modify: `docs/design/02-modules/runtime/09-background-processes.md`
- Modify: `docs/design/02-modules/runtime/04-stuck-prevention.md`
- [x] L5: `background_process_display_pty` 扩 Watch（mock 提取 `bgp_` + `timeout_secs=2` → 断言 `Backend Process Watch` / `still_running`）；强制前台另有 `execution_supervisor_tests::background_process_watch_never_moves_to_background_despite_threshold`

- [ ] **Step 1: 文档**
  - 09：新增 Watch 工具节；写明推翻旧否决、前台强制、timeout、不吞通知
  - 04：注明 Watch **不在** fuse 豁免列及原因

- [ ] **Step 2: 定向验证**

```bash
cargo test -p tools --lib background
cargo test -p runtime --lib wait_task
cargo test -p runtime --lib fuse
cargo test -p cli --lib background_process
cargo clippy -p tools --lib -- -D warnings
```

- [ ] **Step 3: Commit 文档 + 测试补遗**

```bash
git commit -m "$(cat <<'EOF'
docs(runtime): #1931 记录 BackgroundProcessWatch 决策与验收

EOF
)"
```

- [ ] **Step 4: 对照 issue #1931 checklist 逐项勾完或记理由**

---

## 风险

- async_trait 改 port → 所有 stub 必补 `wait_task`
- 不豁免 fuse：连续 Watch 可能 SoftBlock（已接受）
- 双见终态（Watch 结果 + 完成通知）已接受

---

## Follow-ups / 增量（流式 + log_tail）

用户追加（2026-10-12）：Watch TUI 等待中应有 log 流式；结束后给 LLM 的 result 带 log_tail，预算内且不触发 tool_result 截断；含 L4/L5。

- [x] `wait_task` 增量 `read_task_log` → `ToolProgressEvent`（从当前末尾，不回放历史）
- [x] `BackgroundProcessWatchResult.log_tail` + `watch_text` 预算 2000 chars
- [x] TUI `format_result_lines` 渲染 log_tail；L4 pipeline 断言
- [x] L5 PTY：Watch 期间断言 `bytes from` 流式可见
- [x] 单测：`wait_task_emits_tool_progress_for_incremental_log_only`、`watch_tool_result_text_includes_log_tail_within_budget`
