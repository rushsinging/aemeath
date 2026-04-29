# Bug #17 对话进行中无法粘贴

**状态**：✅ 已修复

## 症状
对话进行中（processing/streaming 状态）时，在底部 input area 按 Ctrl+V / Cmd+V 无法粘贴剪贴板内容；空闲状态下粘贴正常。

## 根因
`update.rs` 中 `Msg::Paste` 事件在 `is_processing == true` 时直接丢弃（`Msg::Paste(_) => Cmd::None`）。

## 修复
processing 态下 `Msg::Paste(text)` 新增完整分支：
- 文本粘贴：插入 input area + 入 queued_messages queue
- 空粘贴：尝试剪贴板图片
- 图片路径粘贴：加载图片

## 涉及文件
- `aemeath-cli/src/tui/app/update.rs`
