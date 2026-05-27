# Feature 47 P16: runtime core/ 层端口隔离——消除对外部 crate 的直接引用

> **For agentic workers:** REQUIRED SUB-KILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** runtime `core/` 层不再通过 `crate::api::provider::*` / `crate::api::tools::*` / `crate::api::hook::*` / `crate::api::prompt::*` 等 re-export 间接持有外部 crate 的具体类型。通过端口（port trait）隔离，外部依赖的实现由 application 层注入。

## 当前状态

P15 已将 runtime 内部按 core/business/utils 三层分层，**business/ 层已基本零外部 crate 依赖**（仅剩 2 处可直接清理）。当前外部 crate 耦合集中在 `core/` 层通过 `crate::api::*` 间接引用：

### business/ 残留（可顺手清理）

| crate | 文件 | 行 |
|-------|------|----|
| `provider` | `business/chat/looping/stream_handler.rs` | `use provider::StreamHandler` |
| `storage` | `business/session/mod.rs` | `pub use storage::{` |

### core/ 层通过 crate::api::* 的间接耦合

| 文件 | 引用的具体类型 |
|------|--------------|
| `core/port.rs` | `LlmClient`, `SystemBlock`（provider）、`AgentRunner`, `ToolRegistry`（tools）、`HookRunner`（hook）、`Skill`（prompt）、`TaskStore`（core） |
| `core/tui_launch.rs` | `LlmClient`, `SystemBlock`（provider）、`AgentRunner`, `ToolRegistry`（tools）、`HookRunner`（hook）、`TaskStore`（core）、`SessionReminders`（core） |
| `core/client/from_args.rs` | `SystemBlock`（provider）、`tools as tools_crate`（tools）、`Skill`, `load_all_skills`, `build_system_prompt_parts`, `init_guidance_dir`（prompt）、`ConfigManager`, `TaskStore`, `ToolRegistry`（core） |
| `core/client/accessors.rs` | `LlmClient`（provider）、`McpConnectionManager`（tools）、`HookRunner`（hook） |
| `core/service.rs` | `LlmClient`, `AgentProgressEvent`, `AgentRunner`, `ToolContext`, `ToolRegistry`、`HookRunner` |
| `core/client/trait_command.rs` | `ApiDriverKind`, `LlmClient`, `OpenAIProviderConfig`, `ReasoningConfig` |
| `core/client/mapping.rs` | `Skill`（prompt） |
| `core/command/commands/*.rs` | `crate::business::session::list_sessions()`、`crate::business::reflection::*`、`crate::business::cost::CostTracker`、`crate::business::state::AppState` |

**DDD 原则**：core/（编排层）不应知道外部基础设施细节。Provider、Storage、Hook 是基础设施适配器，core 通过 port（trait）抽象访问。

## 目标架构

```
core/ 层：
  - 定义 port trait（ChatPort、ToolPort、HookPort、PromptPort、ConfigPort、TaskPort）
  - ChatRuntimeContext 从具体类型集合改为 port trait 集合
  - 只依赖这些 trait，不依赖具体 crate 类型

business/ 层：
  - 继续零外部 crate 依赖（仅依赖 share 基础类型 + 内部 port trait）
  - 2 处残留一并清理

infrastructure/ 层（新增或融入现有 utils/）：
  - port trait 的具体实现适配器
```

## 步骤

- [ ] **1. 定义 Port Trait（在 core/port.rs 扩展）**
  - 基于现有 `ChatRuntimePort`，新增细粒度 port：
  - `ProviderPort`：`async fn stream_chat(&self, system, messages, tools, handler, cancel) -> Result<StreamResponse>`
  - `ToolPort`：`async fn execute(&self, call: ToolCall, ctx: &ToolContext) -> ToolResult`
  - `HookPort`：`async fn on_event(&self, event: HookEvent) -> HookDecision`
  - `PromptPort`：`fn build_system_blocks(&self, ctx: &PromptContext) -> Vec<SystemBlock>`
  - `ConfigPort`：`fn resolve_model(&self) -> ModelConfig`、`fn permission_mode(&self) -> PermissionMode`
  - `TaskPort`：`fn store(&self) -> &TaskStore`（或方法代理）
  - 将 `ChatRuntimeContext` 从具体类型集合改为 port trait 集合
  - 每个 port 只暴露 core 需要的最小接口

- [ ] **2. 清理 business/ 层 2 处残留**
  - `business/chat/looping/stream_handler.rs`：`use provider::StreamHandler` → 通过 port 或内部类型替代
  - `business/session/mod.rs`：`pub use storage::{` → 删除或改为 share re-export

- [ ] **3. core/client/from_args.rs 用 port trait 替代具体类型**
  - `SystemBlock` → 通过 `PromptPort::build_system_blocks()` 获取
  - `Skill` / `load_all_skills` → 由 PromptPort 封装
  - `tools as tools_crate` → 通过 ToolPort 访问
  - `ConfigManager`、`TaskStore`、`ToolRegistry` → 通过对应的 ConfigPort / TaskPort / ToolPort

- [ ] **4. core/port.rs + core/service.rs 用 port trait 替代具体类型**
  - `ChatRuntimeContext` 字段：`LlmClient` → `ProviderPort`、`ToolRegistry` → `ToolPort`、`HookRunner` → `HookPort`、`Skill` → `PromptPort`
  - `ChatApplicationService` 相应地改为接受 port trait 集合

- [ ] **5. core/tui_launch.rs 用 port trait 替代具体类型**
  - 过渡字段 `client`, `registry`, `hook_runner`, `system_blocks`, `agent_runner` → port trait 或 SDK DTO
  - 优先将 TUI 需要的运行时对象封装在 port 背后

- [ ] **6. core/client/accessors.rs 用 port trait 替代具体类型**
  - `LlmClient`、`McpConnectionManager`、`HookRunner` → 对应的 port

- [ ] **7. core/client/trait_command.rs 用 port trait 替代具体类型**
  - `ApiDriverKind`、`LlmClient`、`OpenAIProviderConfig`、`ReasoningConfig` → ProviderPort / ConfigPort

- [ ] **8. core/command/commands 中直接调用 business 的改为通过 port / application service**
  - `session.rs` / `stats.rs` → `crate::business::session::list_sessions()` 改为通过 SessionPort 或 application service
  - `reflect.rs` → `crate::business::reflection::*` 改为通过 ReflectionPort
  - `model.rs` → `crate::business::cost::CostTracker` 改为通过 application service
  - `mcp.rs` → `crate::business::state::AppState` 改为通过 port

- [ ] **9. 实现 port trait 的具体适配器（infrastructure/ 或 utils/bootstrap/）**
  - `ProviderAdapter` implements `ProviderPort`（封装 `provider::LlmClient`）
  - `ToolAdapter` implements `ToolPort`（封装 `tools::ToolRegistry`）
  - `HookAdapter` implements `HookPort`（封装 `hook::HookRunner`）
  - `PromptAdapter` implements `PromptPort`（封装 prompt/skill/guidance 逻辑）
  - `ConfigAdapter` implements `ConfigPort`（封装 ConfigManager）
  - `TaskAdapter` implements `TaskPort`（封装 TaskStore）
  - 各 adapter 放在 `agent/runtime/src/utils/adapter/` 或现有 `utils/bootstrap/`

- [ ] **10. from_args() / composition root 注入 port 实现**
  - 在 `AgentClientImpl::from_args()` 中构造各 adapter 并注入

- [ ] **11. 验证**
  - `grep -rn 'crate::api::provider::\|crate::api::tools::\|crate::api::hook::\|crate::api::prompt::' agent/runtime/src/core/` 最多只在 adapter 文件出现
  - `grep -rn 'use provider::\|use tools::\|use storage::\|use hook::\|use prompt::' agent/runtime/src/business/` 返回空
  - `grep -rn 'use provider::\|use tools::\|use storage::\|use hook::\|use prompt::' agent/runtime/src/core/` 返回空（adapter 除外）
  - `cargo build` + `cargo test` 通过
  - `core/` 只依赖 `share::`（基础类型）、port trait 和 `crate::business::`（通过 port 间接）

## 修改记录

- **2026-05-27**：基于 P15 后实际代码状态重写。审计发现 business/ 层已在 P15 中清理至零外部 crate 依赖（仅剩 2 处），将隔离目标从 business/ 层改为 core/ 层；更新所有引用统计数字；增加 core/command 中直接调用 business 的隔离步骤。
- **2026-05-27**：P16 执行完成 (ad29dba)。关键调整：仅需 3 个 port trait 而非 6 个（分析确认 core/ 层大部分类型只是值传递免 port）；步骤 8（core/command 解耦）评估为不属于 P16 范围（core→business 是正常层内调用，非外部 crate 依赖，留待 P17）；最终结果：core/ 层 use provider::/use tools::/use hook::/use prompt:: 直接引用零，201 测试通过，架构守卫全通过。
