# Bug #5: Output Area panic 导致进程卡死

## 基本信息
| 字段 | 值 |
|------|------|
| 编号 | #5 |
| 发现日期 | 2026-04-25 |
| 严重级别 | 高 |
| 状态 | 待修复 |
| 影响范围 | aemeath-cli TUI output_area |

## 症状
Output area 在渲染时触发 panic，由于 panic 被 `catch_unwind` 捕获但未能恢复，导致整个 TUI 进程卡死（无响应，需要 kill）。

## 复现步骤
未确定具体触发条件。已知 output area 的 `Paragraph::render` 已有 `catch_unwind` 包裹，但卡死说明：
1. panic 发生在 `catch_unwind` 之外的代码路径，或
2. `catch_unwind` 捕获后状态不一致导致后续渲染死循环

## 根因分析
待调查。可能原因：
- `screen_line_map` 索引越界
- `CharIdx` 运算溢出
- wrap 计算与 screen_line_map 不一致
- markdown 渲染中的边界条件

## 修复方案
待定。

## 关联
可能与 #3（tool call 行选中）修复引入的 `screen_line_map` 变更有关。
