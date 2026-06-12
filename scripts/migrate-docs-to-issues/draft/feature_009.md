<!-- Migrated from: docs/feature/active.md#9 -->
### #9 反思系统

**状态**：已完成（待确认）

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思，将有价值的经验写入 Memory 系统（#8）。

#### 完成度评估（2026-05-19）

**已实现**：

| 组件 | 位置 |
|------|------|
| `ReflectionEngine`（解析 JSON、格式化输出） | `reflection/` |
| 完整 LLM reflection runner（Prompt 构建、调用 provider、解析结果） | `reflection/runner.rs` |
| Prompt 模板（偏差检测 + 建议记忆 + 过时记忆） | `reflection/prompt.rs` |
| 主循环 N 轮自动触发（`reflection.interval_turns`，0 禁用） | `chat/looping/reflection.rs` |
| Compact 前基于 early messages 提取 memory 建议，默认不自动写入；PostCompact 不再作为目标 | `chat/looping/compact.rs`、`compact/summary.rs` |
| SDK/TUI `/reflect` 调用完整 LLM reflection runner | `packages/sdk/src/tui.rs`、`tui/app/slash/reflection.rs` |
| `/reflect apply` 将 pending 建议写入 MemoryStore | `tui/app/slash/reflection.rs` |
| `auto_apply_suggestions` 自动应用 suggested memories 与 outdated markers | `reflection/apply.rs`、`tui/app/update/ui_event.rs` |
| TUI 自动 reflection 完整展示结果，并保留 pending 建议 | `tui/app/update/ui_event.rs` |
| 手动 `/reflect` 刷新 pending，支持后续 `/reflect apply` | `tui/app/slash/reflection.rs` |

**暂缓/未实现**：连续工具失败、SessionEnd/SubagentStop、独立 reflection model、`ReflectionGenerated` hook、stats/history 持久化。

**说明**：PostCompact 不再作为目标；当前已改为 Compact 前基于即将被压缩的 early messages 提取 memory 建议。

**涉及路径**：`reflection/`、`tui/app/slash/reflection.rs`、`tui/app/update/ui_event.rs`

---
