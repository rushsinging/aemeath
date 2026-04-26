# 鼠标选中时位置错位

## 症状
对话后，在 output area 中用鼠标选中文字时，选择起始点不在鼠标点击的位置，存在偏移。非每次必现，偶尔发生。

## 根因
待确认。可能方向：

1. `screen_line_map` 在渲染时被 trim（`lines` 滚动裁剪），但 `compute_char_offsets` 的宽度计算与 `wrap_line` 不完全对齐
2. 终端 resize 后 `screen_line_map` 未重建，之前渲染的映射与当前坐标不匹配
3. Tool call 行后处理改 dot 颜色时修改了 buffer cell，不影响选中坐标但可能是干扰因素

需要稳定复现后才能缩小范围。

## 修复
待定。

## 回归测试
- 多轮对话后选中文本，验证起始位置与鼠标一致
- 长文本（需 wrapping）中点击中间位置
- 涉及 tool call 行的文本选中

## 关联
- 关联 Bug：#3（tool call 无法选中，已修复）
- 关联 Bug：#1（`screen_line_map` 构建位置修改）
- 涉及路径/模块：screen_line_map `→` compute_char_offsets `→` wrap_line

---
**发现日期**：2026-04-25
**已归档**：待用户确认
