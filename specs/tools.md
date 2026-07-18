# 工具（Tool）系统

**Scope**：`agent/features/tools/**`——`Tool` trait、`ToolRegistry`、内置工具实现、MCP 工具主体。
**主触发**：改 `agent/features/tools/**`。
**次触发**：新增内置 Tool，或改 MCP 工具加载 / 注册。
**配套**：tool 在 Agent 循环中的执行编排在 `runtime.md`；工具调用的 TUI 展示在 `tui-cli.md`。

## Tool trait、注册与 Scope/Profile

- `Tool` / `TypedTool` trait：`agent/features/tools/src/domain/tool.rs`；内置工具实现在 `agent/features/tools/src/adapters/`。
- `ToolRegistry`：`agent/features/tools/src/adapters/tool_registry.rs`，负责工具注册与查找。
- #909 已落地 `RegistryScope` / `ToolProfile` 与“只收缩”规则：`agent/features/tools/src/domain/scope_profile.rs`；capability Published Language：`agent/features/tools/src/domain/published_language.rs`。Main 是 `all()` baseline；Sub 与兼容 `legacy-no-agent` Profile 必须从 Main 经 `derive_restricted` 构造。`TaskRead` 用于 TaskGet/TaskList，变更类 Task tools 使用 `TaskMutation`；LSP 因调用外部 CLI 必须同时声明 `ReadWorkspace | ExecuteProcess`。
- 当前 MCP 动态工具注册仍绕过 `RegistryScopeBuilder` / Scope/Profile；这是 #911 / MCP Ready 的 out-of-scope 差距，不能据 #909 声称全局注册不变量已完成，也不得在 #911 adapter 落地前提前改写 MCP/ToolRegistry 调用链。
- 内置工具的名称、required capabilities、Scope 成员关系与 factory 必须只在 `agent/features/tools/src/adapters/registry.rs` 的单一注册规格中声明。历史 `register_all_tools*` 入口仅作兼容；`NoAgent` 对应 `legacy-no-agent` Scope，等待 #914 退役。
- 异步 trait 方法使用 `async_trait`（见 `rust-coding.md`）。

## ExecutionScope 与最小权限

- `ExecutionScope` 是固定八字段纯值对象：run/parent id、workspace id/root 快照、invocation source、registry scope、profile、deadline；**NEVER** 放入 registry/store/channel/token/semaphore 或 Project wiring。
- `ToolExecutionContext` 只含私有 `scope + ports`。文件工具只经 `WorkspaceRead` 解析路径；`WorkspaceControl` 仅允许 Bash、EnterWorktree、ExitWorktree 使用，accessor 保持 crate-private。
- `WorkspaceViews` 必须在 Runtime adapter 转换；Tools domain 禁止 Tokio channel/token/semaphore。Memory 能力直接使用 #897 发布的正式 `MemoryPort`；不得恢复 legacy compatibility bridge。
- #910 不代表 #911 Catalog/Execution 双 adapter、#877 typed suspension、#912 完整 Runtime scope ownership 已完成。

## MCP 工具

- MCP 主体在 `agent/features/tools/src/adapters/`：`mcp_manager.rs`、`mcp_tool.rs`、`mcp.rs`、`read_mcp_resource.rs`、`list_mcp_resources.rs`。
- MCP 加载器：`agent/features/runtime/src/application/startup/mcp_loader.rs`（`load_mcp_manager`、`parse_mcp_servers_config`、`spawn_mcp_connect`）。
- MCP 外部协议 adapter：`agent/shared/src/adapter/mcp.rs`。
- MCP server 配置来源：`~/.agents/mcp.json`（动态加载，当前通过 `serde_json::Value` 配置——属开放决策）。
- MCP tool 的 description 来自 MCP server 透传，不在内置工具英文化范围内。
