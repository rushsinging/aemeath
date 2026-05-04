# Agent Progress 结构化事件设计

## 背景

Feature #21 当前已把 Agent 子任务进度从工具名列表改成摘要字符串，但协议仍是 `Sender<String>`，TUI 需要解析文本来替换进度行。这会限制后续折叠/展开、分阶段展示、结果状态和耗时展示。

## 目标

把 Agent progress 从字符串协议升级为结构化事件。UI 默认不显示 turn，只展示“当前 Agent 正在做什么”。同一个 Agent 的工具调用进度保持单行更新。

## 非目标

- 本轮不展示 ToolResult、成功/失败、耗时。
- 本轮不实现交互式折叠/展开。
- 不改变 Agent 最终 `ToolResult` 返回路径。

## 数据模型

在 `aemeath-core/src/tool.rs` 增加：

- `AgentProgressEvent { sequence, kind }`
- `AgentProgressKind::ToolCalls { calls }`
- `AgentProgressKind::Message { text }`
- `AgentToolCallProgress { id, name, input, summary }`

`sequence` 仅用于内部定位、测试和未来扩展，TUI 默认不显示。

## 数据流

`CliAgentRunner` 在每轮拿到 `tool_calls` 后，将每个 `ToolCall` 转成 `AgentToolCallProgress`，通过 `Sender<AgentProgressEvent>` 发送到 `AgentTool` 调用上下文。

`aemeath-cli/src/tui/app/stream.rs` 转发事件到 `UiEvent::AgentProgress { tool_id, event }`。

`OutputArea` 根据结构化事件渲染摘要：

```text
↳ Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src
```

同一个 Agent 的 `ToolCalls` 进度行替换旧行，不显示 turn，不重复刷屏。

## 兼容

`AgentProgressKind::Message` 用于兼容普通文本进度。普通消息仍追加显示。

## 测试

- AgentRunner：验证 ToolCall 转结构化事件，包含摘要字段。
- OutputArea：验证同一 Agent 的 ToolCalls 事件替换旧行。
- OutputArea：验证不同 Agent 的 ToolCalls 事件互不覆盖。
- OutputArea：验证 Message 事件保持追加兼容。
