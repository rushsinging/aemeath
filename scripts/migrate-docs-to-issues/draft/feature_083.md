<!-- Migrated from: docs/feature/active.md#83 -->
### #83 Tool result 统一结构化 JSON

**状态**：实现中

**当前进展**：
- `ToolResult` 已增加 `content: serde_json::Value`，`success/error` 默认包装为 `{ "text": ... }`，并保留 `output` 作为 TUI / legacy fallback。
- runtime / sdk / storage oversized 持久化路径已携带 JSON content，`Message::tool_results_rich` 无图片时直接写入结构化 content，有图片时附带 text 与 json block。
- TUI `UiEvent`、conversation model 与 view assembler 已接入结构化 content；`EnterWorktree` / `ExitWorktree` 优先展示 `message` 与 `当前分支：{branch}`，解析失败回退文本。
- `EnterWorktree` / `ExitWorktree` 已返回 `status/message/branch/path_base/working_root/guidance` JSON schema；其他现有工具通过默认构造器统一包装为 `{ "text": ... }`。

**症状 / 目标**：当前工具执行结果在 `ToolResult.output`、runtime stream event、TUI conversation model 中主要以纯文本 `String` 流转；虽然共享消息层的 `ContentBlock::ToolResult.content` 已支持 `serde_json::Value`，但工具层没有统一结构化 payload，导致 LLM 只能收到非结构化文本，TUI 也只能按行截断或做工具名特判。目标是所有 tool result 统一返回 JSON payload：LLM 获得完整结构，TUI 可按工具选择字段展示。

**根因 / 设计点**：
1. `agent/shared/src/tool.rs` 的 `ToolResult` 以 `output: String` 为主，缺少结构化字段。
2. runtime 在 `RuntimeStreamEvent::ToolResult` / `UiToolResult` / `Message::tool_results_rich` 路径中把结果退化为文本。
3. provider conversion 对非 String `content` 已具备 stringify fallback，但当前工具层没有稳定 JSON schema 可依赖。
4. TUI 的 `ToolResultBlockView.result_text` 只保存字符串，缺少按字段展示的统一解析入口。

**实现方向**：
1. 为 `ToolResult` 增加统一结构化 JSON payload，并保留文本 fallback，所有现有工具默认映射为 `{ "text": "..." }`，避免一次性破坏兼容性。
2. 修改 runtime 消息流和发给 LLM 的 tool result 构造逻辑，使 LLM 看到结构化 JSON；provider 层按各 API 能力使用原生 JSON 或 JSON string。
3. 改造所有内置工具返回明确 JSON schema；通用字段建议包括 `status`、`message`、`data`、`diagnostics`、`display`，工具专属字段放入 `data`。
4. `EnterWorktree` / `ExitWorktree` 优先落地结构化 result：保留 `message`、`branch`、`path_base`、`working_root`、路径使用 guidance；TUI 仅展示 `message` 与 `当前分支：{branch}`。
5. TUI 增加结构化 result 展示选择层：优先读取 JSON 中的 display 字段或工具专属字段，解析失败时回退现有纯文本渲染。
6. 更新 session/history/storage 中 tool result 持久化兼容逻辑，确保旧会话纯文本 result 可继续 resume。

**验证**：
- 增加共享 `ToolResult` JSON serialization / fallback 单元测试。
- 增加 runtime tool result → LLM message 的结构化 content 测试。
- 增加 TUI 对结构化 worktree result 的字段选择渲染测试。
- 对代表性工具（文件、bash、搜索、任务、agent、worktree）补充 result JSON schema 回归测试。
- 运行 `cargo fmt --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。

**涉及路径**：
- `agent/shared/src/tool.rs`
- `agent/shared/src/message/*`
- `agent/features/runtime/src/business/chat/looping/*`
- `agent/features/tools/src/**`
- `agent/features/provider/src/business/providers/**/message_conversion.rs`
- `packages/sdk/src/tui.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/tool_call.rs`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/render/output/**`
