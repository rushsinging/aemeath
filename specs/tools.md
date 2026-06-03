# 工具（Tool）系统

**Scope**：`agent/features/tools/**`——`Tool` trait、`ToolRegistry`、内置工具实现、MCP 工具主体。
**主触发**：改 `agent/features/tools/**`。
**次触发**：新增内置 Tool，或改 MCP 工具加载 / 注册。
**配套**：tool 在 Agent 循环中的执行编排在 `runtime.md`；工具调用的 TUI 展示在 `tui-cli.md`。

## Tool trait 与注册

- `Tool` trait：`agent/features/tools/src/contract/tool.rs`。各内置工具实现该 trait。
- `ToolRegistry`：`agent/features/tools/src/core/tool_registry.rs`，负责工具注册与查找。
- 异步 trait 方法使用 `async_trait`（见 `rust-coding.md`）。

## MCP 工具

- MCP 主体在 `agent/features/tools/src/business/`：`mcp_manager.rs`、`mcp_tool.rs`、`mcp.rs`、`read_mcp_resource.rs`、`list_mcp_resources.rs`。
- MCP 加载器：`agent/features/runtime/src/utils/bootstrap/mcp_loader.rs`（`load_mcp_manager`、`parse_mcp_servers_config`、`spawn_mcp_connect`）。
- MCP 外部协议 adapter：`agent/shared/src/adapter/mcp.rs`。
- MCP server 配置来源：`~/.agents/mcp.json`（动态加载，当前通过 `serde_json::Value` 配置——属开放决策）。
- MCP tool 的 description 来自 MCP server 透传，不在内置工具英文化范围内。
