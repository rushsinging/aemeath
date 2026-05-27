# CLI-Agent Tool 事件链路与标题颜色异常分析

## 背景

用户在 TUI 中观察到：某个 tool call 的结果已经出现，说明工具执行已经完成，但 tool call 标题仍未切换到正确的完成颜色，或前缀符号/颜色看起来仍像运行中。

本文整理当前 `agent` 端如何编排 tool 事件、`apps/cli` 如何消费这些事件，以及标题颜色异常的高风险原因和修复方向。

## 结论摘要

当前 CLI 与 Agent 的通信是同进程内的 SDK 事件流：

1. `apps/cli` 通过 `sdk::AgentClient::chat(ChatRequest)` 发起一次 chat turn。
2. `agent/runtime` 启动 `process_chat_loop`，把内部 `RuntimeStreamEvent` 投影为 SDK `ChatEvent`。
3. CLI 后台任务从 `ChatStream` 读取事件，转换为 TUI `UiEvent`。
4. TUI 主循环消费 `UiEvent`，更新 `OutputArea` 中的 tool header、tool result、spinner 与状态栏。

标题颜色异常的主要风险点：

1. `ToolCall` 事件不携带 LLM tool index，CLI 只能按 tool name 找第一个 pending header；同一轮存在多个同名工具时容易绑定错真实 `tool_id`。
2. `ToolResult` 的 fallback 会标记最后一个 running header，可能把别的工具标成完成。
3. `mark_tool_header_done()` 和 `insert_lines_at()` 修改/插入行后没有统一标记 render cache dirty，可能导致数据层状态已变但屏幕仍显示旧样式。
4. `color_tool_call_dots()` 对成功/失败行也强制把首字符画成 `●`，会掩盖 `✓` / `✗` 的真实状态。

## Agent 端事件编排

### 事件类型

runtime 内部事件定义在：

- `agent/runtime/src/business/chat/looping/events.rs`

SDK 暴露给 CLI 的事件定义在：

- `packages/sdk/src/chat.rs`

Tool 相关事件包括：

- `ToolCallStart { name, index }`
- `ToolArgumentsDelta { index, name, partial_args }`
- `ToolCall { id, name, summary }`
- `ToolResult { id, tool_name, output, is_error, images }`
- `AgentProgress { tool_id, event }`
- `WorkingDirectoryChanged { ... }`

其中：

- `ToolCallStart` / `ToolArgumentsDelta` 来自 LLM streaming 阶段，用于显示 pending 占位。
- `ToolCall` 来自 LLM 响应结束后的完整 tool_use 列表，用于把 pending 占位绑定到真实 tool id。
- `ToolResult` 来自工具执行完成后的结果投影，用于插入结果并把 header 标记为成功或失败。

### 单轮 tool round 顺序

核心入口：

- `agent/runtime/src/business/chat/looping/tools.rs::execute_tool_round`

当前顺序：

1. 根据权限拆分 approved / denied tool calls。
2. 对所有 approved tool calls 先发送 `RuntimeStreamEvent::ToolCall`。
3. 按工具类型拆分：
   - `Agent` tool 进入 `execute_agent_calls()`。
   - 其他工具进入 `execute_non_agent()`。
4. 执行工具。
5. 每个工具在完成后发送 `RuntimeStreamEvent::ToolResult`。
6. 最终把 tool results 组装成 API message 继续下一轮 LLM 调用。

注意：第 2 步会先发送所有 `ToolCall`，目的是让 CLI 尽早把 pending 占位替换成正式 header，并写入真实 `tool_id`。

### non-agent tool 执行顺序

核心文件：

- `agent/runtime/src/business/chat/looping/non_agent.rs`

单个 non-agent tool 的执行顺序：

1. `PermissionRequest` hook。
2. `PreToolUse` hook。
3. `agent.execute_tools()` 真正执行工具。
4. 发送 `WorkingDirectoryChanged`。
5. `PostToolUse` hook。
6. Task 相关 hook。
7. 发送 `ToolResult`。

因此，工具实际执行完成后不一定立刻出现 `ToolResult`；hook 事件和 workspace 事件可能夹在中间。

### 多工具并发

`execute_non_agent()` 对 concurrency safe 的工具会并发执行。虽然最终返回给 API 的结果会按位置收集，但 UI 事件是在每个 future 内部发送的。因此 CLI 看到的 `ToolResult` 顺序是完成顺序，不一定是 LLM 发起顺序。

这意味着：

- 多个工具的 `ToolResult` 可以交错。
- `WorkingDirectoryChanged` / hook system message 也可以插入在不同 tool result 之间。
- 并发本身不是错误，但要求 CLI 的 tool header 匹配必须依赖稳定 id 或 index，不能依赖到达顺序。

### Agent tool 进度事件

`Agent` tool 由 `execute_agent_calls()` 处理。Sub-agent 的进度通过独立 progress channel 转发为：

- `AgentProgress { tool_id, event }`

多个 sub-agent 并发时，progress 事件天然会交错。`AgentProgressEventView` 只有单个 agent 内部的 `sequence`，没有全局事件序号。

## CLI 端事件消费

### 事件接入

核心路径：

1. `apps/cli/src/tui/session/processing.rs`
   - 后台任务从 `ChatStream::recv()` 读取 SDK `ChatEvent`。
   - 转换为 TUI `UiEvent`。
   - 发送到 `ui_tx: mpsc::Sender<UiEvent>`。
2. `apps/cli/src/tui/core/run_loop.rs`
   - TUI 主循环从 `ui_rx.recv()` 读取 `UiEvent`。
3. `apps/cli/src/tui/core/update/ui_event.rs`
   - 根据事件更新 `App` 和 `OutputArea`。

### ToolCallStart 消费

处理位置：

- `apps/cli/src/tui/core/update/ui_event.rs`
- `apps/cli/src/tui/output_area/tool_display.rs::push_tool_call_start`

收到：

```text
UiEvent::ToolCallStart { name, index }
```

CLI 会创建 pending header：

```text
● ToolName...
```

其状态为：

- `style = LineStyle::ToolCallRunning`
- `tool_id = pending:{name}:{index}`

该 pending id 来自 LLM streaming 的 tool index，能区分同一轮中的同名工具。

### ToolArgumentsDelta 消费

处理位置：

- `apps/cli/src/tui/output_area/tool_display.rs::update_tool_call_pending`

收到：

```text
UiEvent::ToolArgumentsDelta { index, name, partial_args }
```

CLI 根据：

```text
pending:{name}:{index}
```

精确找到 pending header，并把标题更新成包含参数预览的形式，例如：

```text
● Read(src/lib.rs)
```

这个阶段的匹配是精确的，因为事件携带 `index`。

### ToolCall 消费

处理位置：

- `apps/cli/src/tui/core/update/ui_event.rs`
- `apps/cli/src/tui/output_area/tool_display.rs::push_tool_call`

收到：

```text
UiEvent::ToolCall { id, name, summary }
```

CLI 会尝试把 pending header 替换成正式 header，并把临时 pending id 改为真实 tool id。

当前匹配逻辑是：

```text
查找第一个 tool_id 以 pending:{name}: 开头的 header
```

风险：`ToolCall` 事件没有携带 `index`，所以当同一轮中存在多个同名工具时，CLI 无法知道 `id` 应绑定到哪个 `pending:{name}:{index}`。这会导致 tool id 绑定错位。

### ToolResult 消费

处理位置：

- `apps/cli/src/tui/core/update/ui_event.rs`
- `apps/cli/src/tui/output_area/tool_display/results.rs::push_tool_result_with_diff`
- `apps/cli/src/tui/output_area/tool_display/results.rs::mark_tool_header_done`

收到：

```text
UiEvent::ToolResult { id, tool_name, output, is_error, images }
```

CLI 会：

1. 调用 `mark_tool_header_done(id, tool_name, is_error)`。
2. 把 header 的首字符从 `●` 改为 `✓` 或 `✗`。
3. 把 header 样式改为：
   - `ToolCallSuccess`
   - `ToolCallError`
4. 插入 tool result 行。
5. 从 `active_tool_call_ids` 中移除该 id。
6. 根据剩余 active tool 数量更新 spinner phase。

`mark_tool_header_done()` 的匹配顺序：

1. 精确匹配真实 `tool_id`。
2. fallback：匹配 `pending:{tool_name}:`。
3. last resort：匹配最后一个任意 `ToolCallRunning` header。

该 fallback 设计可以提高容错，但在 id 绑定错位时也可能标错 header。

## 标题颜色异常的具体原因

### 原因一：同名 tool 的 pending header 绑定错位

示例：

```text
ToolCallStart(Read, 0) -> pending:Read:0
ToolCallStart(Read, 1) -> pending:Read:1
ToolCallStart(Read, 2) -> pending:Read:2
```

随后完整 tool_use 事件到达：

```text
ToolCall { id = read-B, name = Read, summary = ... }
```

但事件没有携带 index。CLI 只能找第一个 `pending:Read:`，于是可能把 `read-B` 绑定到 `pending:Read:0`，而真实上它可能对应 `pending:Read:1`。

后续：

```text
ToolResult { id = read-B, tool_name = Read, ... }
```

到达时，CLI 会标记绑定了 `read-B` 的那一行，而用户视觉上认为完成的是另一个 Read header。这会表现为：结果已经出现，但对应标题仍保持 running 颜色。

### 原因二：fallback 标错最后一个 running header

如果真实 id 匹配失败，`mark_tool_header_done()` 会继续 fallback：

1. 找同名 pending。
2. 找最后一个 running header。

在并发和同名 tool 多发场景中，这可能把不相关的 header 标记为成功/失败，真正完成的 header 仍保持 running。

### 原因三：render cache 未统一失效

`mark_tool_header_done()` 会直接修改：

- `line.content`
- `line.style`

但该函数内部没有显式调用：

```text
rendered_cache.content_changed(...)
```

同时，`insert_lines_at()` 插入行时也没有统一触发 cache dirty。

结果可能是：

1. 数据层的 `OutputLine` 已经变为 `ToolCallSuccess`。
2. 渲染缓存仍保存旧的 running 样式。
3. 屏幕继续显示旧颜色，直到后续某个操作触发缓存失效。

这会造成“已经完成但标题颜色没刷新”的视觉现象。

### 原因四：成功/失败符号被 dot overlay 画回 `●`

处理位置：

- `apps/cli/src/tui/output_area/render_status.rs::color_tool_call_dots`

当前逻辑会根据 line style 选择颜色，但最后统一执行：

```text
cell.set_char('●')
```

因此，即使 header 内容已经是：

```text
✓ Bash
```

渲染层也可能把首字符重新画成：

```text
● Bash
```

如果颜色也因为缓存或样式问题未刷新，就会看起来完全仍是运行态。

## 高概率复现场景

1. 同一轮 LLM 同时发起多个同名工具：
   - 多个 `Read`
   - 多个 `Grep`
   - 多个 `Glob`
   - 多个 `Bash`
2. 多个 concurrency safe tool 并发完成。
3. 某个工具执行完成后，hook 或 `WorkingDirectoryChanged` 事件插入在 `ToolResult` 前后。
4. 屏幕处于自动滚动、渲染缓存命中、或者可见窗口包含旧 header 缓存。
5. header 文本已被 pending delta 更新过，但正式 `ToolCall` 绑定到了另一个同名 pending header。

## 建议修复方案

### P0：让 ToolCall 携带 index，精确绑定 pending header

目标：把 `ToolCall` 从按 tool name 绑定改为按 `name + index` 精确绑定。

建议改动：

1. runtime `RuntimeStreamEvent::ToolCall` 增加 `index: usize`。
2. SDK `ChatEvent::ToolCall` 增加 `index: usize`。
3. CLI `UiEvent::ToolCall` 增加 `index: usize`。
4. `push_tool_call()` 接收 index，优先匹配：

```text
pending:{name}:{index}
```

5. 只有 index 不存在或历史兼容路径才 fallback 到同名 pending。

收益：根治同名工具的 tool id 绑定错位。

### P1：收紧 ToolResult fallback

建议：

1. 精确 id 匹配失败时记录 debug 日志，包含：
   - `tool_id`
   - `tool_name`
   - 当前 running headers 列表
2. fallback 到 pending 同名时，也应尽量要求同一 turn 或同一 batch。
3. 最后一个任意 running header fallback 应考虑删除，或只在 debug/兼容模式启用。

收益：避免把错误 header 标成成功，问题更容易暴露和定位。

### P1：统一 render cache 失效

建议：

1. `mark_tool_header_done()` 成功修改 header 后调用 `rendered_cache.content_changed(self.lines.len())`。
2. `insert_lines_at()` 插入行后调用 `rendered_cache.content_changed(self.lines.len())`。
3. 任何直接修改 `OutputLine.content/style/tool_id/spans` 的路径都必须触发 cache dirty。

收益：避免数据层已更新但屏幕仍显示旧样式。

### P2：修正 dot overlay

建议修改 `color_tool_call_dots()`：

1. running 行显示 `●`。
2. success 行显示 `✓`。
3. error 行显示 `✗`。

或者更简单：只调整首字符颜色，不覆盖首字符内容。

收益：避免完成状态被视觉层重新画成运行态。

### P2：增加观测日志

为定位偶发问题，建议在以下位置增加 debug 日志：

1. `push_tool_call_start(name, index)`：记录 pending id。
2. `push_tool_call(id, name, index)`：记录 pending id 到真实 id 的绑定。
3. `mark_tool_header_done(id, tool_name, is_error)`：记录匹配阶段。
4. fallback 命中时记录 warning/debug。

收益：能通过 TUI log 判断是事件乱序、绑定错位，还是渲染缓存问题。

## 修复优先级

1. 先修 `ToolCall` 携带 index 和精确绑定。
2. 同时修 render cache dirty。
3. 再修 dot overlay 的字符覆盖。
4. 最后收紧 fallback 和补充诊断日志。

## 相关文件索引

Agent/runtime：

- `agent/runtime/src/business/chat/looping/events.rs`
- `agent/runtime/src/business/chat/looping/tools.rs`
- `agent/runtime/src/business/chat/looping/non_agent.rs`
- `agent/runtime/src/business/chat/looping/agent_calls.rs`

SDK：

- `packages/sdk/src/chat.rs`
- `packages/sdk/src/client.rs`

CLI/TUI：

- `apps/cli/src/tui/session/processing.rs`
- `apps/cli/src/tui/core/run_loop.rs`
- `apps/cli/src/tui/core/update/ui_event.rs`
- `apps/cli/src/tui/output_area/tool_display.rs`
- `apps/cli/src/tui/output_area/tool_display/results.rs`
- `apps/cli/src/tui/output_area/render_status.rs`
- `apps/cli/src/tui/output_area/rendered_cache.rs`
- `apps/cli/src/tui/output_area/rendered_lines.rs`
