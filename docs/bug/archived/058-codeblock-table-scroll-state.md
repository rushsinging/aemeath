# Bug #58: TUI 中 Markdown 多行代码块/表格滚出视口后渲染异常

- **发现日期**：2026-05
- **归档日期**：2026-05-22
- **状态**：已确认修复
- **优先级**：中

## 症状

1. 多行代码块（```...```）的开标记滚出视口后，结束标记被误认为新开标记，后续所有内容被错误渲染为代码块样式。
2. Markdown 表格的 header/separator 行滚出视口后，数据行不被识别为表格，退化为普通文本。
3. 表格滚动时列宽随可见内容变化（因列宽计算只基于传入的可见行，未包含完整表格）。

## 根因

`scan_code_blocks`、`scan_table_blocks` 和 `render_table_cache`（`render_blocks.rs`）只扫描当前可见行：

- 代码块：开标记滚出视口后 `in_code_block` 初始为 `false`，结束标记变成新开标记。
- 表格：header + separator 滚出后，可见数据行无 separator，不被识别为表格。
- 列宽：`render_table_block` 的 `column_widths` 只基于传入行计算，视口外更宽的单元格未参与，导致滚动时列宽抖动。

## 修复

- `scan_code_blocks`：从文档开头预扫描到可见区域起始位置，确定 `in_code_block` 初始状态（提交 `dac5759`）。
- `scan_table_blocks`：预扫描不可见部分，检测跨越视口边界的表格块（提交 `ed77d0e`）。
- `render_table_cache`：收集整个表格块的全部行（向前向后均查找），传给 `render_table_block` 计算列宽后只取可见行渲染结果（提交 `48f8eed`）。
- 测试移至 `render_blocks_tests.rs`，7 个单元测试覆盖正常路径、滚出视口回归、无分隔符误识别等场景。
