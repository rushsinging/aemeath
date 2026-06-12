<!-- Migrated from: docs/feature/active.md#28 -->
### #28 MCP 系统完善

**目标**：完善 MCP 系统的配置、连接管理、工具发现注册和 CLI 操作路径，覆盖本轮 P0+P1 基础能力；P2 不在本轮范围，待后续单独规划。

#### 本轮已完成的 P0+P1 改动

- `McpServerConfig` 支持 stdio 可用配置；SSE/Streamable HTTP 仅完成配置解析与 URL 安全校验，传输实现仍为占位存根；header 脱敏已完成。
- McpConnectionManager 接入启动加载路径，统一管理连接/tool 发现/注册。
- ToolRegistry 注销与查询接口。
- Manager tool diff、snapshot refresh、`health_check_once` API 和状态转换；未暗示后台定时 health check loop 已启动。
- `/mcp` 独立命令模块已落地，支持 list/tools/restart/add/remove 的解析与预览输出；实际运行时操作待 runtime bridge 接入。
- MCP tool result 默认 1MB 响应大小限制。

#### 已知限制

- SSE 传输存在可靠性问题：z.ai 等远程 MCP server 的 SSE response 在 tools/list 时经常出现超时或不完整（2890 bytes 后 SSE stream 停止发送，缺少 `\n\n` 终结符）。已尝试 POST body 消费、独立 client、incomplete event fallback 等方案，均未根本解决。
- **MCP 加载已从 TUI 启动流程中禁用**（`run_orchestration.rs` 中 `spawn_mcp_connect` 调用已注释），避免启动时因 MCP 连接超时阻塞。
- `/mcp` restart/add/remove/list/tools 暂未接入 runtime manager bridge，仅返回状态/预览提示。
- Streamable HTTP 传输未实现。
- 健康检查已有单次 API，后台定时 loop 尚未接入。

#### 待修复方向

- SSE stream 可靠性：考虑改用 Streamable HTTP 传输（单 POST 请求返回完整 JSON-RPC response，不依赖 SSE event stream）。
- 或者在 SSE transport 中增加更健壮的重试机制和超时策略。
- 需要测试更多 MCP SSE server 以确认是 z.ai 特定问题还是通用问题。

#### 关联

- Feature #28 MCP P0+P1 实施任务 1-9 已完成并 review 通过
- 用户确认后再归档；确认前保留在 active.md

---
