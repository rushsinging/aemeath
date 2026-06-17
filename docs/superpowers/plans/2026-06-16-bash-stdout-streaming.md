# Bash stdout 流式推送到 TUI 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bash 工具执行过程中 stdout 实时流式显示到 TUI（Issue #273 要求 2）

**Architecture:** 复用现有 AgentProgress 通道（`progress_tx` → `AgentProgressEvent::Message` → `activity_summary` 渲染）。Bash 工具在读取 stdout chunk 时，通过 `ctx.progress_tx` 发送 `AgentProgressEvent::Message` 事件；`non_agent.rs` 像 `agent_calls.rs` 一样为每次工具调用设置 channel 并转发到 sink。TUI 端的 `activity_summary` 渲染机制已完整，无需新增事件类型。

**Tech Stack:** Rust, tokio mpsc channel, ratatui TUI

**对应 Issue:** rushsinging/aemeath#273

---

## 文件结构

| 文件 | 职责 | 改动类型 |
|------|------|----------|
| `agent/features/runtime/src/business/agent/agent.rs` | Agent 工具执行器 | 新增 `execute_one_with_ctx` 公开方法 |
| `agent/features/tools/src/business/bash.rs` | Bash 工具实现 | stdout 读取循环中发送 chunk |
| `agent/features/runtime/src/business/chat/looping/non_agent.rs` | 非 Agent 工具执行编排 | 设置 progress channel + 直接调用 |

---

## Task 1: 在 Agent 上新增 `execute_one_with_ctx` 方法

**Files:**
- Modify: `agent/features/runtime/src/business/agent/agent.rs`（在 `execute_tools` 方法之后，约 L255 前）

**背景：** 当前 `non_agent.rs` 通过 `agent.execute_tools()` 执行工具，该方法内部使用 `&self.ctx`（不可变引用），无法注入 `progress_tx`。需要新增一个接受外部 context 的单工具执行方法，复用已有的 `call_tool_with_timeout` 超时/取消逻辑。

- [ ] **Step 1: 在 `Agent` impl 块中新增 `execute_one_with_ctx` 方法**

在 `agent.rs` 的 `impl Agent<'_>` 块中（`execute_tools` 方法之后、`execute_tools_filtered` 之前），添加：

```rust
/// Execute a single tool call with a custom context (for streaming support).
///
/// Mirrors the sequential path of `execute_tools` but allows the caller to
/// inject a modified `ToolExecutionContext` (e.g., with `progress_tx` set
/// for stdout streaming). Reuses `call_tool_with_timeout` for timeout/cancel.
pub async fn execute_one_with_ctx(
    &self,
    call: &ToolCall,
    ctx: &ToolExecutionContext,
) -> ToolResultTuple {
    if ctx.cancel.is_cancelled() {
        return (
            call.id.clone(),
            call.provider_id.clone(),
            "Cancelled by user".to_string(),
            serde_json::json!({ "text": "Cancelled by user" }),
            true,
            Vec::new(),
        );
    }
    if let Some(tool) = self.registry.get(&call.name) {
        match call_tool_with_timeout(tool, &call.name, call.input.clone(), ctx).await {
            Ok(result) => (
                call.id.clone(),
                call.provider_id.clone(),
                result.output,
                result.content,
                result.is_error,
                result.images,
            ),
            Err(message) => (
                call.id.clone(),
                call.provider_id.clone(),
                message.clone(),
                serde_json::json!({ "text": message }),
                true,
                Vec::new(),
            ),
        }
    } else {
        (
            call.id.clone(),
            call.provider_id.clone(),
            format!("unknown tool: {}", call.name),
            serde_json::json!({ "text": format!("unknown tool: {}", call.name) }),
            true,
            Vec::new(),
        )
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo build -p aemeath-runtime`
Expected: 编译通过，无错误

- [ ] **Step 3: Commit**

```bash
git add agent/features/runtime/src/business/agent/agent.rs
git commit -m "feat: add Agent::execute_one_with_ctx for context-injected tool execution"
```

---

## Task 2: Bash 工具通过 `progress_tx` 流式发送 stdout chunk

**Files:**
- Modify: `agent/features/tools/src/business/bash.rs`

**背景：** Bash 的 stdout 读取循环在独立 tokio task 中运行，将数据累积到 `Vec<u8>`。需要在读取每个 chunk 时额外通过 `ctx.progress_tx` 发送 `AgentProgressEvent::Message`，使 TUI 实时看到输出。

- [ ] **Step 1: 添加 share::tool 导入**

在 `bash.rs` 文件顶部的 `use` 区域（L3 附近），添加：

```rust
use share::tool::{AgentProgressEvent, AgentProgressKind};
```

- [ ] **Step 2: 在 stdout 读取 task 中发送 chunk**

将 stdout 读取 task（当前 L117-135）从：

```rust
let stdout_handle = tokio::spawn(async move {
    let mut buf = Vec::new();
    if let Some(ref mut pipe) = stdout_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    // If over limit, keep reading (to drain the pipe) but don't store
                }
                Err(_) => break,
            }
        }
    }
    buf
});
```

替换为（在 task 创建前克隆 `progress_tx`，在循环内发送 chunk）：

```rust
let progress_tx = ctx.progress_tx.clone();

let stdout_handle = tokio::spawn(async move {
    let mut buf = Vec::new();
    if let Some(ref mut pipe) = stdout_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    // If over limit, keep reading (to drain the pipe) but don't store

                    // Stream stdout chunk to TUI via progress_tx
                    if let Some(tx) = &progress_tx {
                        let text = String::from_utf8_lossy(&tmp[..n]).to_string();
                        let _ = tx.try_send(AgentProgressEvent {
                            sequence: 0,
                            kind: AgentProgressKind::Message { text },
                        });
                    }
                }
                Err(_) => break,
            }
        }
    }
    buf
});
```

**关键设计点：**
- `ctx.progress_tx` 是 `Option<mpsc::Sender<AgentProgressEvent>>`，`Sender` 实现了 `Clone`，所以 `ctx.progress_tx.clone()` 可以在 `tokio::spawn` 前 move 进 task
- `try_send` 是非阻塞的：如果 channel 满了（TUI 消费慢），直接丢弃该 chunk（不影响最终结果，最终结果由 `ToolResult` 事件完整传递）
- `String::from_utf8_lossy` 处理 chunk 边界的多字节字符（替换字符可接受，因为是实时预览）
- `sequence: 0`：该字段仅用于内部排序，UI 不显示（见 `AgentProgressEvent` 定义注释）

- [ ] **Step 3: 编译验证**

Run: `cargo build -p aemeath-tools`
Expected: 编译通过，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/features/tools/src/business/bash.rs
git commit -m "feat: stream bash stdout chunks via progress_tx to TUI"
```

---

## Task 3: `non_agent.rs` 设置 progress channel 并直接调用工具

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/non_agent.rs`（`execute_one_non_agent` 函数，L177-262）

**背景：** 当前 `execute_one_non_agent` 通过 `agent.execute_tools()` 执行工具，该方法使用 agent 内部 context（无 `progress_tx`）。需要改为：设置 channel → spawn 转发 task → 用注入了 `progress_tx` 的 context 直接调用工具 → 清理。这与 `agent_calls.rs::execute_one_agent`（L110-151）的模式完全一致。

- [ ] **Step 1: 修改 `execute_one_non_agent` 中的工具执行段落**

将当前的 `execute_tools` 调用段落（约 L237-248）：

```rust
      let exec_results = agent.execute_tools(std::slice::from_ref(&owned_call)).await;
      let working_root = agent.ctx.workspace_read().current_root();
      let in_worktree = agent.ctx.workspace_read().in_worktree();
      hook_runner.set_project_context(working_root.display().to_string(), in_worktree);
      let workspace = project::api::WorkspacePersist::snapshot(agent.ctx.workspace.as_ref());
      let _ = sink
          .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
              path_base: workspace.path_base.clone(),
              working_root: workspace.working_root.clone(),
              workspace,
          })
          .await;
```

替换为：

```rust
      // Set up progress channel for stdout streaming (mirrors agent_calls.rs pattern).
      // The channel allows tools (e.g., Bash) to send real-time stdout chunks to the TUI
      // via the existing AgentProgress event pipeline.
      let (prog_tx, mut prog_rx) =
          tokio::sync::mpsc::channel::<share::tool::AgentProgressEvent>(32);
      let mut streaming_ctx = agent.ctx.clone();
      streaming_ctx.progress_tx = Some(prog_tx);
      let call_id = owned_call.id.clone();
      let stream_sink = sink.clone();
      let stream_context = context.clone();
      let forward_handle = tokio::spawn(async move {
          while let Some(event) = prog_rx.recv().await {
              let _ = stream_sink
                  .send_event(RuntimeStreamEvent::AgentProgress {
                      context: stream_context.clone(),
                      tool_id: call_id.clone(),
                      event,
                  })
                  .await;
          }
      });

      let exec_results = vec![agent.execute_one_with_ctx(&owned_call, &streaming_ctx).await];

      // Flush any remaining progress events before proceeding to post-tool hooks.
      let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

      let working_root = agent.ctx.workspace_read().current_root();
      let in_worktree = agent.ctx.workspace_read().in_worktree();
      hook_runner.set_project_context(working_root.display().to_string(), in_worktree);
      let workspace = project::api::WorkspacePersist::snapshot(agent.ctx.workspace.as_ref());
      let _ = sink
          .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
              path_base: workspace.path_base.clone(),
              working_root: workspace.working_root.clone(),
              workspace,
          })
          .await;
```

**关键设计点：**
- `streaming_ctx` 是 `agent.ctx` 的 clone，注入了 `progress_tx`；`workspace` 字段是 `Arc<WorkspaceService>`，与 `agent.ctx` 共享同一状态，所以工具内的 `set_cwd` 等修改对两者都可见
- `forward_handle` 转发 task 在工具执行期间持续将 `AgentProgressEvent` 转为 `RuntimeStreamEvent::AgentProgress` 发到 sink
- 500ms timeout 确保即使 channel 中有残留事件也能被 flush，与 `agent_calls.rs:151` 一致
- `exec_results` 类型从 `Vec<ToolResultTuple>` 变为 `vec![ToolResultTuple]`，后续代码 `for (id, provider_id, output, content, is_error, images) in exec_results` 无需修改（元组结构一致）

- [ ] **Step 2: 编译验证**

Run: `cargo build -p aemeath-runtime`
Expected: 编译通过，无错误

- [ ] **Step 3: Commit**

```bash
git add agent/features/runtime/src/business/chat/looping/non_agent.rs
git commit -m "feat: inject progress_tx into non-agent tool execution for stdout streaming"
```

---

## Task 4: 为 Bash stdout 流式推送添加单元测试

**Files:**
- Modify: `agent/features/tools/src/business/bash.rs`（`#[cfg(test)] mod tests` 区域）

- [ ] **Step 1: 添加测试验证 progress_tx 接收到 stdout chunk**

在 `bash.rs` 测试模块末尾（L351 之后，`}` 闭合 `mod tests` 之前）添加：

```rust
    #[tokio::test]
    async fn test_bash_streams_stdout_via_progress_tx() {
        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let (tx, mut rx) = tokio::sync::mpsc::channel::<share::tool::AgentProgressEvent>(32);
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: Some(tx),
            parent_session_id: None,
        };

        let result = BashTool
            .call(json!({ "command": "echo streaming_test_output" }), &ctx)
            .await;

        assert!(!result.is_error);

        // Collect all progress events sent during execution
        let mut messages = Vec::new();
        while let Ok(event) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if let share::tool::AgentProgressKind::Message { text } = event.kind {
                messages.push(text);
            }
        }

        // At least one chunk should contain the echoed output
        let combined = messages.join("");
        assert!(
            combined.contains("streaming_test_output"),
            "progress_tx should have received stdout chunk containing 'streaming_test_output', got: {}",
            combined
        );
    }

    #[tokio::test]
    async fn test_bash_no_progress_tx_still_works() {
        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        };

        let result = BashTool
            .call(json!({ "command": "echo no_streaming" }), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("no_streaming"));
    }
```

- [ ] **Step 2: 运行测试验证通过**

Run: `cargo test -p aemeath-tools -- bash`
Expected: 所有 bash 测试通过，包括新增的 2 个测试

- [ ] **Step 3: Commit**

```bash
git add agent/features/tools/src/business/bash.rs
git commit -m "test: add unit tests for bash stdout streaming via progress_tx"
```

---

## Task 5: 全量验证门禁

- [ ] **Step 1: 完整编译**

Run: `cargo build`
Expected: 编译通过

- [ ] **Step 2: Clippy 检查**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 无 warning

- [ ] **Step 3: 全量测试**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 4: 手动 TUI 验证**

启动 TUI，执行一个会产生持续输出的 Bash 命令（如 `for i in $(seq 1 5); do echo "line $i"; sleep 0.5; done`），观察执行过程中 TUI 是否实时显示 stdout 输出行。

Expected: 执行过程中能看到 "line 1"、"line 2"... 依次出现；命令完成后 activity 行消失，ToolResult 子块展示完整输出。
