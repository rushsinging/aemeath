# Bug #24 Tool call 执行时 spinner 偶尔消失

**状态**：✅ 已修复，用户已确认
**发现日期**：2026-04
**确认日期**：2026-05-01
**优先级**：中
**根因类别**：tool call 状态、spinner 生命周期或 reserved height 计算偶发不同步

## 症状

模型正在执行 tool call 时，TUI 底部 spinner 偶尔消失或短暂不显示。实际 tool call 仍在执行，用户会误以为界面卡住或请求结束。

## 根因

1. `tool_call_active` 是单个 bool，不能表达多个并发/批量 tool call。Agent tool 批量执行时，第一个 `ToolResult` 把 `tool_call_active` 置为 false 并切回 `Generating...`，但其他 tool call 可能仍在运行，spinner 生命周期与真实 tool 执行状态不同步。
2. Output area 临时行追加顺序为 queued messages → spinner → task status lines。task status lines 较多时，最终裁剪优先保留底部 task 行，spinner 可能被挤出可见区域。

## 修复

1. `App` 新增 `active_tool_call_ids: HashSet<String>`，`UiEvent::ToolCall` 记录未完成 tool id，`UiEvent::ToolResult` 只移除对应 id；仅当 active set 为空时才把 tool 状态切回 `Generating...`。
2. Error / Cancelled / Done / DoneWithDuration / 新一轮 processing 开始时清空 active set，避免跨轮残留。
3. Output area 渲染顺序改为 queued messages → task status lines → spinner，让 spinner 永远位于临时区域最后一行。
4. 补充修复（2026-04-28）：`ToolResult` 当 `remaining == 0` 时改为 `start_spinner()` 而非仅设置状态栏文字，确保 agent loop 进入下一轮 API 调用期间 spinner 持续显示；`update_task_status` 移除每帧 `start_spinner()` 调用。日志标签从 `[BUG#4]` 统一更新为 `[SPINNER]`。

## 涉及文件

- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/output_area/mod.rs`
- `aemeath-cli/src/tui/output_area/spinner.rs`

## 验证

用户已确认修复。
