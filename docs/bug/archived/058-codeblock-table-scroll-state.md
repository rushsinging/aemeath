# Bug #58: TUI 中 Markdown 多行代码块/表格滚出视口后渲染异常

- **发现日期**：2026-05
- **归档日期**：2026-05-23
- **状态**：已确认修复
- **优先级**：中

## 症状

1. 多行代码块（```...```）的开标记滚出视口后，结束标记曾被误认为新开标记，后续所有内容被错误渲染为代码块样式。
2. Markdown 表格的 header/separator 行滚出视口后，数据行曾不被识别为表格，退化为普通文本。
3. 表格滚动时列宽曾随可见内容变化。
4. 后续用户再次反馈：Markdown table 的 header 部分滚出可视区域后，表格头部/相关区域仍会局部退回 Markdown 原文，且 code 渲染状态仍可能丢失。

## 根因

早期实现中，`scan_code_blocks`、`scan_table_blocks` 和 `render_table_cache` 主要围绕当前可见行恢复状态：

- 代码块：开标记滚出视口后 `in_code_block` 初始状态可能错误，结束标记会被当成新开标记。
- 表格：header + separator 滚出后，可见数据行缺少完整表格上下文，可能不被识别为表格。
- 列宽：列宽计算只基于传入行，视口外更宽的单元格未参与。

后续复发的根因是渲染仍缺少稳定的跨视口 block 上下文：当可见窗口从 Markdown block 中间开始时，table/code 等 block 的起始状态和完整 block 边界没有统一纳入渲染输入，导致局部 fallback 到原始 Markdown 文本或样式丢失。

## 修复

- 第一轮修复：
  - `scan_code_blocks`：从文档开头预扫描到可见区域起始位置，确定 `in_code_block` 初始状态（提交 `dac5759`）。
  - `scan_table_blocks`：预扫描不可见部分，检测跨越视口边界的表格块（提交 `ed77d0e`）。
  - `render_table_cache`：收集整个表格块的全部行，传给 `render_table_block` 计算列宽后只取可见行渲染结果（提交 `48f8eed`）。
- 复发后的最终修复：
  - 新增 `rendered_cache.rs`，集中管理渲染缓存，并将滑动窗口在前后各扩展 50% 后继续扩展到 Markdown block 边界，保证 table/code 等 block 在渲染时拥有完整上下文。
  - 新增 `rendered_lines.rs`，抽出行渲染函数，避免可见区裁剪与 Markdown block 渲染状态交织。
  - `render.rs` 改为通过缓存层获取渲染结果，再按实际视口裁剪显示，避免 header 或 code fence 滚出后局部回退。
  - `table.rs` 支持按可用宽度自动换行列内容，减少表格宽度变化和截断带来的显示异常。

## 验证

用户已确认 bug #58 修复。活动列表中移除 #58，并保留此归档记录。
