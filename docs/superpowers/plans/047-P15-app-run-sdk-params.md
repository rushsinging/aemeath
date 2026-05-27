# Feature 47 P15: App::run() 参数 SDK 化

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `App::run()` 签名从 18 个 runtime 类型参数收束为 `AgentClient` + 少量原始类型，`TuiLaunchContext` 不再暴露 runtime 内部类型给 CLI。

## 背景

当前 `App::run()` 接收 18 个参数，其中大量为 runtime 类型：
- `Arc<LlmClient>`、`Arc<ToolRegistry>`、`Vec<SystemBlock>`、`Arc<dyn AgentRunner>`、`Arc<TaskStore>`、`Arc<Semaphore>` 等
- 实际在 `run()` 内部只使用了 `client`（判断 reasoning）、`context_size`、`allow_all`、`resume_id`、`task_store`

`run_orchestration.rs` 从 `TuiLaunchContext` 逐字段拆出赋值给 App，字段达 21 个。

## 步骤

- [ ] **1. SDK `AgentClient` 新增辅助查询方法**
  - `fn is_reasoning(&self) -> bool` — runtime 侧读 `current_client.is_reasoning()`
  - `fn context_size(&self) -> usize` — runtime 侧返回 `context.context_size`
  - `fn allow_all(&self) -> bool` — runtime 侧返回 `context.allow_all`
  - `fn model_display(&self) -> &str` — runtime 侧返回 resolved model display

- [ ] **2. 改写 `App::run()` 签名**
  - 从 18 参数改为：
    ```rust
    pub async fn run(&mut self, client: Arc<dyn AgentClient>, resume_id: Option<String>) -> io::Result<()>
    ```
  - 内部通过 `client.is_reasoning()` / `client.context_size()` 等获取需要的值

- [ ] **3. `TuiLaunchContext` 收束为 SDK DTO**
  - 删除 `TuiLaunchContext` 中的 runtime 类型字段（`client`、`registry`、`system_blocks`、`agent_runner`、`task_store`、`hook_runner`、`json_logger` 等）
  - 只保留 SDK 类型 + 原始类型（`session_id`、`cwd`、`model_display`、`memory_config`、`skills_map`）
  - 或直接删除 `TuiLaunchContext`，用 `AgentClient` trait 方法替代

- [ ] **4. 简化 `run_orchestration.rs`**
  - 删除 `launch.xxx` 逐字段赋值
  - 改为：
    ```rust
    let client = ::runtime::api::client::from_args(args.into()).await?;
    let session_id = client.session_id().to_string();
    let mut app = App::new(session_id, client.cwd().into(), client.model_display().into());
    app.agent_client = Some(Arc::new(client.clone()));
    app.run(Arc::new(client), initial_resume_id).await
    ```

- [ ] **5. 迁移 TUI 内部的 client 引用**
  - `status_bar.set_thinking(client.is_reasoning())` → `client.is_reasoning()`
  - `status_bar.set_context_size()` → `client.context_size()`
  - 其他内部字段通过 AgentClient 方法访问

- [ ] **6. 验证**
  - `cargo build -p cli` 编译通过
  - `cargo test -p cli` 通过
  - TUI 启动、聊天、退出流程正常
