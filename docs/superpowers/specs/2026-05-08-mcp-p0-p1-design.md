# MCP P0+P1 设计规格

## 背景

Feature #28 的目标是把当前 MCP 骨架升级为可用的 MCP 系统。当前代码已经具备 stdio 客户端、启动加载、MCP tool 动态注册、资源读取工具，以及一个尚未接入主流程的 `McpConnectionManager`。本次范围选择完整 P0+P1：主流程接入 Manager、运行时管理、`/mcp` 命令、基础安全、SSE / Streamable HTTP、健康检查、自动重连、tool 热刷新。

本次不实现 P2：Prompts、Sampling、完整 TUI MCP 状态面板。

## 目标

1. MCP 生命周期由 `McpConnectionManager` 统一管理，不再由 `mcp_loader.rs` 直接连接并注册 tool。
2. 支持 stdio、SSE、Streamable HTTP 三种传输配置。
3. 支持健康检查、自动重连、重连后的 tool 重新发现。
4. 支持 `notifications/tools/list_changed` 触发工具列表热刷新。
5. `/mcp` 命令从占位升级为可查询与可操作的命令集。
6. 增强基础安全：远程 URL 校验、敏感 header 脱敏、tool response 大小限制。

## 非目标

1. 不实现 MCP Prompts 工具。
2. 不实现 MCP Sampling 反向 LLM 调用。
3. 不实现复杂 TUI 弹窗审批；首次连接审批与 tool 调用审批仅保留配置结构和安全校验边界，不做交互式 UI。
4. 不重构整个 `ToolRegistry`，若现有 registry 不支持注销，则热刷新中的删除采用“Manager 标记不可用 + 下次 schema 构建不暴露”的最小实现路径。

## 架构

### 传输层

`aemeath-core/src/mcp.rs` 保留 `McpClient` 对外 API，但内部通过传输抽象发送 JSON-RPC。传输抽象提供：

- `send_request(method, params) -> Value`
- `send_notification(method, params)`
- `is_alive()` 或 `ping()` 支持健康检查

配置通过 `McpServerConfig` 表达：

- stdio：`command`、`args`、`env`
- 远程：`url`、`headers`、`transport`

`transport` 可取 `stdio`、`sse`、`streamable_http`。未显式配置时：有 `command` 走 stdio，有 `url` 默认走 streamable_http。

### Manager

`aemeath-core/src/mcp_manager.rs` 成为 MCP 生命周期中心：

- 读取合并后的 `McpManagerConfig`
- 初始化 server connection 状态
- connect / disconnect / reconnect
- tools/list 发现工具
- 注册 MCP Tool
- health loop 定时 ping
- tool list changed 通知后刷新工具列表

主流程中 `main.rs` 创建 Manager 并调用初始化入口。`aemeath-cli/src/mcp_loader.rs` 只负责从 `.mcp.json`、`~/.aemeath/config.json` 的 `mcp` 段、`~/.aemeath/mcp.json` 构造 Manager 配置，不再直接操作 `McpClient`。

### Tool 注册与热刷新

MCP tool 名称保持 `mcp__<server>__<tool>`。Manager 维护 server → tools 快照。热刷新时重新拉取 tools/list，对比旧快照：

- 新增 tool：注册到 `ToolRegistry`
- 已存在 tool：覆盖 Manager 快照
- 删除 tool：Manager 标记为 unavailable；若 `ToolRegistry` 支持注销，则从 registry 删除，否则 wrapper 调用时返回“tool no longer available”

### `/mcp` 命令

新增 `aemeath-core/src/command/commands/mcp.rs`，从 `tools.rs` 中移除占位逻辑。命令能力：

- `/mcp`：列出 server、状态、工具数、错误摘要
- `/mcp tools [server]`：列出工具名与描述
- `/mcp restart <server>`：重启 server
- `/mcp add <name> ...`：添加 server 配置
- `/mcp remove <name>`：断开并移除配置

若当前 command 执行上下文无法直接持有 Manager，先实现命令解析与状态输出接口，并通过 runtime bridge 把操作请求交给 CLI 层执行。

### 安全

- stdio command 继续要求绝对路径，拒绝 shell 元字符与 shell/interpreter。
- 远程 MCP URL 默认只允许 `https://`；`http://localhost`、`http://127.0.0.1`、`http://[::1]` 允许用于本地开发。
- 日志输出 headers 时必须脱敏 `Authorization`、`Cookie`、`X-Api-Key` 等敏感键。
- tool response 默认限制为 1MB，超过后截断并返回明确提示。

## 数据流

1. 启动时加载 config。
2. `mcp_loader` 合并 MCP 配置并创建 `McpConnectionManager`。
3. Manager 初始化 connections。
4. Manager 连接 server，initialize 后发现 tools。
5. Manager 将 MCP tools 注册到 `ToolRegistry`。
6. LLM 调用 `mcp__server__tool`。
7. wrapper 通过 Manager / client 调用 server。
8. health loop 定时 ping，失败时自动 reconnect。
9. 收到 tools/list_changed 后刷新工具快照并更新 registry。

## 错误处理

- 连接失败：server 状态设为 `Failed`，记录错误，其他 server 不受影响。
- ping 失败：状态设为 `Reconnecting`，按配置重试。
- 重连失败达到上限：状态设为 `Failed`，保留错误信息。
- tool 调用失败：返回 `ToolResult::error`，不 panic。
- 配置解析失败：跳过单个 server 并记录 warning，不阻断启动。
- 远程 URL 不安全：拒绝连接，状态设为 `Failed`。

## 测试策略

1. `McpServerConfig` 解析：stdio、sse、streamable_http、非法组合。
2. 传输选择：command 优先 stdio，url 默认 streamable_http，显式 sse 生效。
3. URL 安全校验：https 通过，公网 http 拒绝，localhost http 通过。
4. Manager 状态转换：Initializing → Connected、Failed、Reconnecting。
5. tool 热刷新 diff：新增、删除、描述变更。
6. `/mcp` 命令解析：list、tools、restart、add、remove。
7. 编译验证：`cargo check`。

## 验收标准

1. `main.rs` 不再绕过 Manager 直接加载 MCP tools。
2. stdio MCP 仍可从 `.mcp.json` 加载并注册工具。
3. SSE / Streamable HTTP 配置能通过解析与传输选择测试。
4. Manager 能执行 ping 失败后的自动重连状态转换。
5. tool list changed 能触发工具快照刷新逻辑。
6. `/mcp` 输出真实 server 状态，不再是静态占位文本。
7. `cargo check` 通过。
