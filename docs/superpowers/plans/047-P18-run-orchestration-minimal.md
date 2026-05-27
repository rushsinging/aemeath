# Feature 47 P18: run_orchestration 极简化

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `run_orchestration.rs` 瘦身到 ~40 行：解析参数 → 创建 AgentClient → 构造 App → 调用 `App::run`。删除 `runtime_adapter.rs`。

## 背景

P15 完成后，`App::run()` 只接受 `Arc<dyn AgentClient>` + `resume_id`。`run_orchestration.rs` 应该变得极简。

当前 102 行中：
- `permission_env_override` / `apply_permission_env_override` — 环境变量权限覆盖
- `initial_tui_resume_id` — 提取 resume ID
- `run_chat` — 23 行初始化 + 46 行 App 字段赋值 + `app.run()` 调用

`runtime_adapter.rs`（17 行）只有 `create_agent_client` 和 `set_current_turn`，P14 后 `set_current_turn` 走 SDK，`create_agent_client` 可内联。

## 步骤

- [ ] **1. 确认 P14/P15 已完成**
  - `CmdExecutor` 只持有 `Arc<dyn AgentClient>`
  - `App::run()` 只接受 `Arc<dyn AgentClient>` + `resume_id`

- [ ] **2. 简化 `run_chat` 函数**
  - 目标：
    ```rust
    pub(crate) async fn run_chat(args: Args) {
        ::runtime::api::command::commands::init_all();
        let args = apply_permission_env_override(args);
        let resume_id = args.resume.clone();
        let client = ::runtime::api::client::from_args(args.into())
            .await
            .unwrap_or_else(|e| { eprintln!("Error: {e}"); std::process::exit(1); });
        let session_id = client.session_id().to_string();
        let mut app = App::new(session_id, client.cwd().into(), client.model_display().into());
        app.agent_client = Some(Arc::new(client.clone()));
        app.run(Arc::new(client), resume_id).await
            .unwrap_or_else(|e| { log::error!("TUI error: {e}"); std::process::exit(1); });
        println!("aemeath --resume {}", session_id);
    }
    ```

- [ ] **3. 删除 `runtime_adapter.rs`**
  - `set_current_turn` 已走 SDK（P14）
  - `create_agent_client` 直接在 `run_chat` 中调用 `from_args`

- [ ] **4. `init_all()` 移入 runtime 内部**
  - 将 `::runtime::api::command::commands::init_all()` 调用移到 `from_args()` 内部，CLI 不再显式调用

- [ ] **5. `main.rs` 检查**
  - 确认 `main.rs` 中 `::runtime::api::bootstrap::init_panic_hook()` 也通过 SDK 暴露或移入 `from_args()`

- [ ] **6. 验证**
  - `cargo build -p cli` 编译通过
  - TUI 启动 / 聊天 / 退出正常
  - `--resume` 正常
