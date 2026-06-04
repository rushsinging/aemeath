# Bug #102：长工具调用内容导致 TUI 画面完全不刷新、按键无响应

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 270a8c7 |

## 症状

执行包含超大参数或结果的工具调用期间，TUI 画面完全不刷新，spinner 不动，键盘输入/快捷键无响应；表现为 event loop 被同步重活堵住，而不是单纯停留在 Generating 状态。高风险工具包括 Write 大 `content`、Edit 大 `old_string/new_string`、Agent 大 `prompt`、Bash 大 `command`，以及 Read/Grep/Glob/Bash/WebFetch/Agent 等大 result。

## 根因

工具 I/O 多数走异步路径，不应直接阻塞 TUI 主线程。更可疑的是 TUI 渲染/update 路径保存和处理完整工具参数或完整工具结果，导致每帧发生大字符串 clone、`lines()` 全量收集、宽度计算、block cache version/hash 计算或富文本渲染，从而阻塞 event loop。

## 修复

1. TUI 层展示工具调用时只保留路径、字节数、小预览等摘要，NEVER 将完整大字段放入可反复 clone/render/hash 的 view model。
2. 所有工具结果进入 TUI model 前按字节上限截断；工具结果预览按 `result_max_lines` streaming/take 截断，NEVER 为了显示前 N 行先 `collect()` 完整 result lines。
3. 添加大工具参数/结果回归测试，覆盖格式化/渲染路径不会随正文大小线性处理完整正文。

## 验证

- 用户确认修复。

## 涉及路径

- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- `apps/cli/src/tui/render/output/blocks/tool_result.rs`
- `apps/cli/src/tui/view_assembler/output.rs`

## 关联提交

- `270a8c7 fix(tui): 统一截断大工具内容预览 (refs #102)`
