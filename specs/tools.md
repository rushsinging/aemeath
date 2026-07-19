# 工具（Tool）系统

**Scope**：`agent/features/tools/**`——Tool Published Language、Catalog/Execution 端口、内置与 MCP adapter。
**主触发**：改 `agent/features/tools/**`。
**次触发**：新增内置 Tool，或改 MCP 工具加载 / 注册。
**配套**：tool 在 Agent 循环中的执行编排在 `runtime.md`；工具调用的 TUI 展示在 `tui-cli.md`。

## Catalog / Execution 与 Scope/Profile

- `ToolCatalogPort` / `ToolExecutionPort`：`agent/features/tools/src/domain/ports.rs`；Descriptor、Invocation、Outcome 等 Published Language：`agent/features/tools/src/domain/published_language.rs`。
- #911 已把生产 Runtime 切到 Catalog / Execution 双端口：Runtime 不再取得 `ToolRegistry` 或 `Tool` 实例。两端由 `agent/features/tools/src/adapters/composition.rs` 的窄 factory 基于同一个私有 `ToolBacking` 装配；具体 backing、registry 与 adapter 不从 crate root 暴露。
- Catalog 按 Scope/Profile 投影 schema；Execution 在调用时复验 Tool 存在性、Scope/Profile 授权并执行。schema 校验实现唯一归 Tools：`agent/features/tools/src/domain/schema_validator.rs`；Runtime 的 `application/agent/input_validation.rs` 仅保留兼容 re-export / phase peel，不得复制规则。
- `RegistryScope` / `ToolProfile` 与“只收缩”规则：`agent/features/tools/src/domain/scope_profile.rs`。Main 是 `all()` baseline；Sub 必须由 Main 经 `derive_restricted` 构造。内置工具名称、required capabilities、Scope 成员关系与 factory 的单一规格在 `agent/features/tools/src/adapters/registry.rs`。
- `ToolRegistry` 当前仍是 Tools adapter 内部 backing；`legacy-no-agent`、历史 `register_all_tools*`、内部 Profile/Registry 与 `SkillTool` 的最终物理退役属于 #914，不能把 #911 解读为这些旧实现已清零。
- 异步 trait 方法使用 `async_trait`（见 `rust-coding.md`）。

## ExecutionScope、资源与 suspension

- `ExecutionScope` 是固定八字段纯值对象：run/parent id、workspace id/root 快照、invocation source、registry scope、profile、deadline；**NEVER** 放入 registry/store/channel/token/semaphore 或 Project wiring。
- `ToolExecutionContext` 只含私有 `scope + ports`。文件工具只经 `WorkspaceRead` 解析路径；`WorkspaceControl` 仅允许 Bash、EnterWorktree、ExitWorktree 使用，accessor 保持 crate-private。
- `WorkspaceViews` 必须在 Runtime adapter 转换；Runtime 自持 `WorkspacePersist`、并发 semaphore、timeout、Policy/Hook 与等待机制，Tools domain 禁止 Tokio channel/token/semaphore。Memory 能力直接使用正式 `MemoryPort`，不得恢复 legacy compatibility bridge。
- AskUser adapter 只解析并返回纯值 `ToolSuspension::UserInteraction`：`agent/features/tools/src/domain/suspension.rs`、`agent/features/tools/src/adapters/ask_user.rs`。request id、waiter、continuation、await/resume 与取消归 Runtime；#911 只完成 typed suspension 边界及生产映射，不代表 #877/#878 的完整 Interaction 状态机完成。
- #912/#913 的 Runtime/Composition ownership 与装配收口仍未完成。

## MCP 工具

- MCP 主体在 `agent/features/tools/src/adapters/`：`mcp_manager.rs`、`mcp_tool.rs`、`mcp.rs`、`read_mcp_resource.rs`、`list_mcp_resources.rs`。
- MCP 加载器：`agent/features/runtime/src/application/startup/mcp_loader.rs`；MCP 外部协议 adapter：`agent/shared/src/adapter/mcp.rs`。
- #911 只提供保守 dynamic-source seam：动态 callable 可进入私有 backing，但不会因此自动获得 Scope 成员资格或 Profile 授权。它不改变既有连接生命周期。
- MCP server 配置来源：`~/.agents/mcp.json`（动态加载，当前通过 `serde_json::Value` 配置——属开放决策）；description 来自 MCP server 透传。
- MCP Ready 的显式连接生命周期、disconnect 撤销/refresh、Catalog revision、稳定身份与版本协议均未完成，不得从 source seam 推导 Ready。
