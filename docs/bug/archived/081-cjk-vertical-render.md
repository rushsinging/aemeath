# Bug #81：TUI 输出区中文按单字竖排显示

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染宽度首帧 layout 未就绪 |

## 症状

进入/恢复 TUI 后，上一条 assistant 中文内容被按单字拆成多行显示，例如"理 / 一 / 轮 / ， / 不 / 改 / 代 / 码 / 。"；同屏后续 system-reminder 和工具输出仍能正常横向显示。

## 根因

#58 输出区渲染管线切到 `ConversationModel -> OutputViewModel -> OutputDocumentRenderer` 后，`refresh_output_widget_from_model` 使用 `layout.output_area_rect.width.saturating_sub(2).max(1)` 作为渲染宽度。首次进入/恢复会话时，frame 尚未 draw，`output_area_rect` 仍是默认 `Rect::default()`，于是渲染宽度变成 1；CJK 字符显示宽度为 2，markdown wrap 在 width=1 下每字符独立成行，形成逐字竖排。

## 修复

`render_document_from_view_model` 在传入 layout width 未就绪（<=1）时，不再直接用 1 渲染，而是回退到 `OutputArea` 已知的 `term_width`。resize 已提供终端宽度但首帧 layout rect 尚未更新时，assistant 中文文本仍按正常宽度渲染。

## 回归测试

1. `test_assistant_cjk_text_does_not_wrap_per_character_at_normal_width`
2. `test_render_document_from_view_model_uses_known_term_width_when_layout_width_unready`

## 相关提交

- `d15614c` fix: 修复中文输出逐字竖排 (refs #81)

## 验证

2026-05-30 用户确认 bug #81 已修复。
