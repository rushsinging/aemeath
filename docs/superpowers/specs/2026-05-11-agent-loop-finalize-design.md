# Agent Loop 收尾工作 — 设计文档

**日期**：2026-05-11  
**Feature**：#30  
**状态**：设计完成，待实施

## 概述

把 agent loop（主 loop 在 `stream.rs`、子 agent loop 在 `agent_runner.rs`）的所有退出路径收敛到统一 finalize，消除各路径各自手写清理逻辑导致的行为不一致。

## 当前问题

### 主 agent loop（stream.rs）6 个退出路径

| # | 退出路径 | 退出方式 | 清理 |
|---|---------|---------|------|
| 1 | 正常完成（EndTurn） | break → 公共尾部 | reflection、Stop hook、DoneWithDuration |
| 2 | 重复输出 stall | break → 公共尾部 | 同 #1 |
| 3 | 工具循环 stall | break → 公共尾部 | 同 #1 |
| 4 | 用户打断 | **return** | Cancelled、Done，**缺 Stop hook / agent_loop_finished** |
| 5 | API 错误 | **return** | Error、StopFailure hook、Done，**缺 agent_loop_finished** |
| 6 | Ctrl+C | → 变 #4 或 #5 | — |

退出方式分两类：`break`（走公共尾部）和 `return`（跳过公共尾部）。打断和 API 错误用 `return`，导致公共尾部的 `Stop` hook 和 `agent_loop_finished` 不可达。

### 子 agent loop（agent_runner.rs）5 个退出路径

| # | 退出路径 | 清理 |
|---|---------|------|
| 1 | 取消 | SubagentStop hook + restore client |
| 2 | 超时 | SubagentStop hook + restore client |
| 3 | 正常完成 | SubagentStop hook + restore client |
| 4 | API 错误 | SubagentStop hook + restore client |
| 5 | Max turns | SubagentStop hook + restore client |

子 agent 已相对统一（都调 SubagentStop + restore client），但各自手写，且缺少结构化日志摘要。

## 设计

### 1. 新增类型

放在 `aemeath-cli/src/agent_runner.rs`。

```rust
/// Agent 循环退出状态
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AgentRunStatus {
    Completed,           // 正常完成（EndTurn / 无 tool calls / stall）
    Cancelled,           // 用户打断（Ctrl+C / Escape）
    TimedOut,            // 子 agent 超时（10 分钟硬限制）
    ApiError(String),    // LLM API 错误
    MaxTurns,            // 子 agent 达到 max turns
}

/// Agent 循环统一结果
#[derive(Debug, Clone)]
pub(crate) struct AgentRunOutcome {
    pub status: AgentRunStatus,
    pub turns: usize,
    pub duration: Duration,
    pub role: Option<String>,  // 子 agent 有 role，主 loop 为 None
    pub model: String,
}
```

### 2. 共用逻辑：`log_agent_outcome`

提炼自 `stream.rs:1215-1225` 现有的 `agent_loop_finished` JSON 日志。主 loop 和子 agent 都调用。

```rust
fn log_agent_outcome(outcome: &AgentRunOutcome, session_id: &str)
```

写入结构化 JSON 日志（status、turns、duration、role、model），复用现有 `JsonLogger`。

### 3. 主 loop 统一

**核心策略**：打断（#4）和 API 错误（#5）不再 `return`，改为设置 `outcome` 后 `break`，走到公共尾部。

**公共尾部**（当前 L1215-1239 改造）：

```rust
async fn finalize_main_loop(
    outcome: &AgentRunOutcome,
    tx: &Sender<UiEvent>,
    hook_runner: &HookRunner,
    session_id: &str,
) {
    log_agent_outcome(outcome, session_id);

    match outcome.status {
        AgentRunStatus::Completed | AgentRunStatus::MaxTurns => {
            hook_runner.run_hooks(HookEvent::Stop, ...).await;
            tx.send(UiEvent::DoneWithDuration(outcome.duration)).await;
        }
        AgentRunStatus::Cancelled => {
            tx.send(UiEvent::Done).await;
        }
        AgentRunStatus::ApiError(_) | AgentRunStatus::TimedOut => {
            hook_runner.run_hooks(HookEvent::StopFailure, ...).await;
            tx.send(UiEvent::Done).await;
        }
    }
}
```

各退出路径只负责：
1. 设置 `outcome`（含正确的 status）
2. 执行路径特有前置处理（如打断时的 truncate + Cancelled UI 事件）
3. `break`

不再有 `return` 跳过尾部。

### 4. 子 agent finalize

```rust
async fn finalize_sub_agent(
    outcome: &AgentRunOutcome,
    client: &LlmClient,
    hook_runner: &HookRunner,
    prompt: &str,
    system: &str,
    model_spec: Option<&str>,
    output: &str,
    session_id: &str,
) {
    log_agent_outcome(outcome, session_id);
    hook_runner.on_subagent_stop(
        prompt, system, model_spec,
        output, outcome.turns,
        outcome.status != AgentRunStatus::Completed,
    ).await;
    restore_client_settings(client);
}
```

子 agent 内 5 个 return 路径改为：构造 outcome → 调 `finalize_sub_agent` → return。

### 5. 不做的事

- 不自动完成 pending task
- 不启发式更新 task 状态
- 不改 `AgentRunner` trait 签名（仍返回 `String`）
- 不改 `is_agent_failure()` 字符串匹配（后续单独改进）

## 涉及文件

| 文件 | 改动 |
|------|------|
| `aemeath-cli/src/agent_runner.rs` | 新增 `AgentRunStatus`、`AgentRunOutcome`、`log_agent_outcome`、`finalize_sub_agent`；5 个 return 路径改用 finalize |
| `aemeath-cli/src/tui/app/stream.rs` | 新增 `finalize_main_loop`；打断/错误路径 `return` → `break`；公共尾部改为调 finalize |

## 测试

单元测试覆盖：
- `log_agent_outcome` 各 status 变体的日志格式
- 主 loop finalize 各 status 的 hook 触发和 UI 事件
- 子 agent finalize 各 status 的 SubagentStop hook 调用和 client 恢复

## 关联

- Bug #27/#29（task 状态不更新）— 已有修复，#30 不重复
- Bug #34（task batch summary）— 已有修复，#30 不重复
- Feature #25（task 跨轮次生命周期）— #30 不做自动归档
