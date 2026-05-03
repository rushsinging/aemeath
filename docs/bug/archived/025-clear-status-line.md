# Bug #25 /clear 命令未清空 status line 数据

**状态**：✅ 已修复，用户已确认
**发现日期**：2026-04
**确认日期**：2026-05-01
**优先级**：中
**根因类别**：clear 仅重置消息历史，未联动重置 status bar 任务/成本/spinner 状态

## 症状

在 TUI 中执行 `/clear` 命令清空对话历史后，output area 的消息已经被清空，但底部 status line / status bar 仍然显示上一轮残留信息：
- task 汇总（`✓` / `■` / `□` + subject 行）
- cost / token 用量数字
- spinner 状态或 "Generating..." / "Calling xxx..." 文本
- 当前 tool call 标题

用户预期 `/clear` 不仅清空消息列表，也应该把状态栏复位回初始空白态（保留模型 / provider / cwd 等环境信息，清掉本会话累计的运行态）。

## 根因

`/clear` 命令的 handler 只调用了 output_area 清空接口，未联动重置 status bar 与 App 的运行态字段。task list、active tool calls、spinner 状态、当前 tool call 名等动态字段作为独立字段持有，缺乏统一的 reset 入口。

## 修复

- App 暴露统一的运行态复位入口，`CommandAction::Clear` 在清空消息历史的同时调用复位逻辑
- 复位字段：active_tool_call_ids、当前 tool call 名、spinner 状态、task summary 行
- 不复位：model、provider、cwd 等环境信息；cost / token 累计保留（session 级数据）

## 涉及文件

- `aemeath-core/src/command/commands/`（`/clear` 命令实现）
- `aemeath-cli/src/tui/app/update.rs`（`CommandAction::Clear` 分支处理）
- `aemeath-cli/src/tui/app/mod.rs`（App 状态字段）
- `aemeath-cli/src/tui/status_bar.rs`（status bar 渲染来源）
- `aemeath-cli/src/tui/output_area/mod.rs`

## 关联

- Bug #24（spinner 生命周期）— 复位逻辑同时清空 active tool call set

## 验证

用户已确认修复。
