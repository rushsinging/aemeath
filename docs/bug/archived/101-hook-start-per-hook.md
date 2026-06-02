# Bug #101：HookUi 只发一次 HookStart，多 hook 场景下 spinner 只显示第一个 hook 命令

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-02 |
| 状态 | 已确认修复 |
| 修复 | 4c81141, 7a78d65 |

## 症状

配置多个 Stop hook 时，TUI spinner 只显示第一个 hook 的命令名，后续 hook 执行期间没有对应 UI 反馈。

## 根因

`hook_ui.rs` 中 `HookStart` 只在执行前发送一次，并且只取 `hooks.first().command`；实际 hook runner 会串行执行多个匹配 hook，但每个 hook 执行前没有逐个通知 UI。

## 修复

将 HookStart 通知调整为每个 hook 执行前单独发送，使 CLI/TUI 能按当前正在运行的 hook 更新 spinner 文案。

## 验证

- 相关 hook runner / runtime 测试通过。
- 用户确认修复。
