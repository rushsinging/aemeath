# Bug #41 执行 /reflect 时 TUI 短暂卡死后才出现 LLM 输出

- **状态**：已归档
- **确认日期**：2026-05-18
- **确认结果**：用户确认修复
- **优先级**：高
- **发现日期**：2026-05

## 症状

执行 `/reflect` 时，TUI 会短暂卡死，等待一段时间后才出现 LLM 输出。

## 根因

`/reflect` 在 `run_loop` 中同步 `await` LLM 调用，阻塞了 TUI 主事件循环，导致界面无法及时刷新和响应输入。

## 修复

`/reflect` 不再在 `run_loop` 中同步等待 LLM；改为后台 task 执行，并通过 `UiEvent` 回传 `started`、`usage`、`done` 等状态。

修复后 TUI 会立即显示 spinner，并在 LLM 执行期间保持响应。
