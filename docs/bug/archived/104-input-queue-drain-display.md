# Bug #104: input queue drain 后没有在 TUI 中显示

**状态**: 已修复 (已确认)
**优先级**: 中
**发现日期**: 2026-06

## 根因

processing 期间 Enter 走 InputEventPort 而非 QueueDrainPort，runtime drain 后发 MessagesSync 只更新 chat.messages 和清除排队块，但没有将新增 user messages 渲染到 conversation model。

## 修复

MessagesSync 中比较新旧 messages，用 append_user_echo 回显新增 user messages。
