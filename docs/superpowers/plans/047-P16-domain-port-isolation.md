# Feature 47 P16: runtime domain 层 supporting domain 委托——消除直接 crate 操作

> **For agentic workers:** REQUIRED SUB-KILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** runtime `domain/` 层不再直接 `use provider::` / `use tools::` / `use storage::` / `use hook::`，通过端口（port trait）隔离。supporting domain 的操作统一由 `application/` 层编排注入。

## 当前状态

runtime `domain/` 层直接操作 supporting domain crate 的统计：
- `provider::` — 53 处引用（LlmClient 直接调用、provider 类型）
- `tools::` — 11 处（ToolRegistry 直接使用）
- `storage::` — 14 处（session 持久化直接调用）
- `hook::` — 36 处（HookRunner 直接调用）
- `prompt::` — 9 处（Skill/Guidance 加载）
- `share::` — 73 处（共享类型，可接受）

**DDD 原则**：domain 层不应知道外部基础设施细节。Provider、Storage、Hook 是基础设施适配器，domain 层通过 port（trait）抽象访问。

## 目标架构

```
domain 层：
  - 定义 port trait（LlmStreamPort、ToolExecutionPort、SessionPersistencePort、HookPort、PromptPort）
  - 只依赖这些 trait，不依赖具体 crate

application 层：
  - 注入 port trait 的具体实现（provider crate、tools crate 等）
  - 编排 domain 对象之间的交互

infrastructure 层：
  - port trait 的具体实现适配器
```

## 步骤

- [ ] **1. 定义 Port Trait**
  - 在 `domain/` 下创建 `ports.rs`（或 `domain/ports/mod.rs`）
  - `LlmStreamPort`：`async fn stream_chat(&self, req: LlmRequest) -> Result<LlmStream, DomainError>`
  - `ToolExecutionPort`：`async fn execute_tool(&self, call: ToolCall, ctx: &ToolContext) -> Result<ToolResult, DomainError>`
  - `SessionPersistencePort`：`async fn save(&self, session: &SessionRecord) -> Result<(), DomainError>` + `async fn load(&self, id: &str) -> Result<SessionRecord, DomainError>`
  - `HookPort`：`async fn on_notification(&self, ...) -> Result<(), DomainError>`
  - `PromptPort`：`fn build_system_prompt(&self, ctx: &PromptContext) -> Vec<SystemBlock>`
  - 每个 port 只暴露 domain 需要的最小接口

- [ ] **2. domain/agent 用 LlmStreamPort 替代 provider:: 直接调用**
  - `agent_runner/loop_run.rs`、`chat/looping/stream_handler.rs` 中 `provider::LlmClient` → `LlmStreamPort`
  - `chat/looping/agent_calls.rs` 同上

- [ ] **3. domain/chat 用 ToolExecutionPort 替代 tools:: 直接调用**
  - `chat/looping/tools.rs` 中 `tools::ToolRegistry` → `ToolExecutionPort`

- [ ] **4. domain/session 用 SessionPersistencePort 替代 storage:: 直接调用**
  - `session/storage.rs` → port 注入

- [ ] **5. domain/chat 用 HookPort 替代 hook:: 直接调用**
  - `chat/looping/hook_ui.rs`、`chat/looping/finalize.rs` → `HookPort`
  - `bootstrap/` 中的 HookRunner 构造 → infrastructure 层适配器

- [ ] **6. domain/prompt 用 PromptPort 替代 prompt:: 直接调用**
  - `prompt_build/` → port 注入或移入 infrastructure 适配器

- [ ] **7. infrastructure/ 中实现 port trait 的具体适配器**
  - `ProviderAdapter` implements `LlmStreamPort`（封装 `provider::LlmClient`）
  - `ToolAdapter` implements `ToolExecutionPort`（封装 `tools::ToolRegistry`）
  - `StorageAdapter` implements `SessionPersistencePort`（封装 `storage::SessionStore`）
  - `HookAdapter` implements `HookPort`（封装 `hook::HookRunner`）
  - `PromptAdapter` implements `PromptPort`（封装 `prompt::SkillCatalog` + `GuidanceLoader`）

- [ ] **8. application 层注入 port 实现**
  - `from_args()` 中构造各 adapter 并注入到 domain 对象

- [ ] **9. 验证**
  - `grep -rn 'provider::\|tools::\|storage::\|hook::\|prompt::' agent/runtime/src/domain/` 返回空
  - `cargo build` + `cargo test` 通过
  - `domain/` 只依赖 `share::`（core）和 port trait
