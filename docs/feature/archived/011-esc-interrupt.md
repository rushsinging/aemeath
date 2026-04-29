# #11 Esc 打断对话

**状态**：✅ 已完成

## 目标
对话进行中（streaming/tool call）时按 Esc 即可打断本轮，与 Ctrl+C 行为一致。

## 实现

### Esc 打断（input_handler.rs + update.rs）
- **空闲态**：Esc 保持原行为（清除 suggestions）
- **processing 态**：Esc 触发与 Ctrl+C 相同的取消路径
  - `spawn_refs.interrupted.store(true)` 
  - `active_cancel.lock()` → `token.cancel()`
  - `status_bar.set_warning("Interrupted")`

### 附加修复：Paste 在 processing 态下支持
- `update.rs` 中 `Msg::Paste(text)` 新增 processing 态分支
- 文本粘贴：插入 input area + 入 queued_messages queue
- 空粘贴：尝试剪贴板图片
- 图片路径粘贴：加载图片

## 涉及文件
- `aemeath-cli/src/tui/app/input_handler.rs` — Esc 打断
- `aemeath-cli/src/tui/app/update.rs` — Esc 打断 + Paste processing 态
