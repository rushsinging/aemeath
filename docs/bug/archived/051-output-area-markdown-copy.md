# Bug #51：Output area 复制时复制出 Markdown 源码而非渲染后纯文本

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 状态 | 已确认修复 |
| 确认日期 | 2026-05-23 |
| 根因类别 | TUI 选择复制 / Markdown 渲染坐标 |

## 症状

Output area 中拖选复制 Markdown 内容时，复制结果包含 Markdown 源码标记，或在行内代码、链接等标记参与坐标换算时出现文本截断。例如复制渲染后的 `活动中 Bug（docs/bug/active.md）` 时，曾得到缺少右括号的 `活动中 Bug（docs/bug/active.md`。

## 根因

早期修复只在 `get_selected_text` 返回前剥离 Markdown 标记，但 selection_start / selection_end 仍按原始 Markdown 文本坐标计算。TUI 实际渲染时会剥离 `**bold**`、`*italic*`、行内代码和链接等标记，导致渲染坐标与原始内容坐标不一致。

当用户按渲染后的可见文本拖选时，选区坐标映射回原始 Markdown 内容会发生偏移，最终复制出 Markdown 源码或截断文本。

## 修复

Markdown 普通行渲染时写入渲染后纯文本覆盖，并基于该纯文本构建 `screen_line_map` 和选区坐标。这样复制逻辑使用的坐标与用户看到的渲染文本一致。

同时补充“活动中 Bug（`docs/bug/active.md`）”相关回归测试，覆盖行内代码 / 链接标记参与坐标换算时的复制结果。

## 关键提交

| commit | 说明 |
|--------|------|
| `c27b42d` | 修复 Markdown 渲染文本与选择复制坐标不一致问题 |

## 确认结果

用户已于 2026-05-23 确认该问题修复。
