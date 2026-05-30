# TUI 滚动位置固定（Scroll Pin）

日期：2026-05-30

## 问题

用户向上滚动查看历史内容时（`auto_scroll=false`），新生成的 AI 回复内容会在底部追加，
导致视窗下移，用户正在看的内容被推出视野。

## 根因

`scroll_offset` 语义为"距底部 X 行"。当底部有新内容追加时，底部下移 Δ 行，但 `scroll_offset`
不变，视窗窗口随之整体下移同样行数，用户原来看着的第 N 行被推到视窗以上。

## 方案

在渲染前管线 `apply_output_scroll_to_widget` 中，检测文档行数增长，当
`auto_scroll=false` 时将增长量 Δ 补偿到 `scroll_offset`，使视窗窗口的顶部行号
在内容增长前后保持一致。

## 涉及文件

| 文件 | 变更 |
|---|---|
| `apps/cli/src/tui/view_state/output.rs` | 新增 `last_document_total_lines: usize` |
| `apps/cli/src/tui/adapter/output_view_widget.rs` | `apply_output_scroll_to_widget`：增长补偿逻辑 |
| `apps/cli/src/tui/view_state/output.rs` (tests) | 新增测试：增长补偿、收缩钳制 |

## 核心逻辑变更

`apply_output_scroll_to_widget` 调整后的顺序：

1. 反喂 `last_visible_height`（不变）
2. **新增**：计算文档行数增长量 `growth = new_total - last_document_total_lines`；
   若 `!auto_scroll && growth > 0`，则 `scroll_offset += growth`
3. 更新 `last_document_total_lines = new_total`
4. 钳制 `scroll_offset` 到 `max_offset`（不变）
5. offset 为 0 时恢复 `auto_scroll`（不变）
6. 写回 widget 镜像（不变）

## 边界情况

- **`auto_scroll=true`（贴底模式）**：不触发补偿，行为不变
- **内容收缩**（thinking 块移除等）：`growth` 为 0（`saturating_sub`），不补偿；
  后续钳制逻辑处理 offset 越界
- **首帧（初始状态）**：`last_document_total_lines=0`，growth 可能极大——但此时
  `auto_scroll=true`，不触发补偿
- **用户滚回底部**：`scroll_offset` 归零时 `auto_scroll` 自动恢复为 `true`

## 测试计划

### 新增 `OutputViewState` 测试

- `test_scroll_pin_growth_compensates_offset`：内容增长时 `auto_scroll=false`，
  offset 应增加 `growth`
- `test_scroll_pin_shrink_clamps_without_panic`：内容收缩时 offset 超出新的
  max_offset，应被钳制且不 panic
- `test_scroll_pin_auto_scroll_true_skips_compensation`：贴底模式下不触发补偿
- `test_last_document_total_lines_default_zero`：新增字段默认值为 0

### 新增 `apply_output_scroll_to_widget` 测试

- `test_apply_compensates_for_content_growth_when_not_auto_scroll`：模拟 50 行内容
  增长到 60 行的场景，验证 offset 增加了 10
- `test_apply_no_compensation_when_auto_scroll`：auto_scroll=true 时不补偿
