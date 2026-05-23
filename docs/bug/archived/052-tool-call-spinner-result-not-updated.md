# Bug #52：Tool call spinner 一直闪烁且 tool 结果未更新

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 状态 | 已确认修复 |
| 确认日期 | 2026-05-22 |
| 根因类别 | TUI 状态同步 / tool_id 映射 |

## 症状

当一轮 LLM 响应同时包含已批准和被拒绝的 tool call 时，output area 中被拒绝的 tool call（如 `● Edit...`）前面的白点持续闪烁，且拒绝结果（如 "Tool Edit denied"）未在该 tool call 下方正确展示。

## 复现路径

1. `allow_all` 关闭（默认），LLM 发起一个 Approved 工具（如 Read）+ 一个 Denied 工具（如 Edit）
2. 流式阶段两个 tool call 均创建 `pending:{name}:{index}` 占位行
3. `deny_tool_calls` 对 Denied 工具仅发送 `ToolResult` 事件（携带 LLM 原生的 `tool_use_id`）
4. `push_tool_result_with_diff` → `mark_tool_header_done(tool_use_id)` 精确匹配失败（占位行的 tool_id 是 `pending:Edit:0`，不是 `tool_use_id`）
5. 触发 fallback → 抓最后一个 `ToolCallRunning` 行（可能是 Read 的占位行），错误地标记 Read 为完成
6. Edit 的占位行 `pending:Edit:0` 永远保持 `ToolCallRunning` → 白点持续闪烁

## 根因

`deny_tool_calls` 只发送 `ToolResult` 事件，不发送 `ToolCall` 事件，导致 pending placeholder 的 `tool_id`（格式 `pending:{name}:{index}`）永远无法被 `mark_tool_header_done` 的精确匹配阶段命中。

同时，fallback 的“抓最后一个 `ToolCallRunning`”逻辑在同轮存在多个 running tool 时会抓错行。

## 修复

1. `deny_tool_calls` 对每个被拒绝的 call 先发送 `UiEvent::ToolCall`，让 `push_tool_call` 将占位行的 tool_id 更新为 LLM 的 `tool_use_id`，再发送 `ToolResult`。
2. `mark_tool_header_done` fallback 从单阶段改为三阶段：
   - Phase 1：精确 tool_id 匹配
   - Phase 2：pending 占位行前缀匹配（`pending:{tool_name}:`）
   - Phase 3：最后兜底为任意 `ToolCallRunning` 行
3. 补充回归测试覆盖 pending 精确 fallback 与 exact tool_id 优先级。

## 关键提交

| commit | 说明 |
|--------|------|
| `617c168` | fix(#52): deny_tool_calls 发送 ToolCall 事件 + mark_tool_header_done 三阶段 fallback |
| `deae072` | test(#52): cover tool result pending fallback |
| `fc41127` | Merge branch 'fix/bug-52-tool-spinner' |

## 涉及路径

- `apps/cli/src/tui/app/stream/tools.rs`
- `apps/cli/src/tui/output_area/tool_display/results.rs`

## 确认结果

用户已于 2026-05-22 确认该问题修复。
