# Bug #78：input area 粘贴后按空格清空粘贴内容

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 输入数据流方向 / paste 路径与 model 不一致 |

## 症状

在 TUI input area 粘贴一段内容后，再按空格，粘贴内容会被清空，只剩刚按下的空格。

## 根因

同 #77 根因：`handle_paste_event` 与 processing 模式 paste 均直接调用 `input_area.input(ch)` 修改 textarea，未走 InputModel。后续空格触发 `model.apply(InsertChar)` → `TextChanged` → `set_text`，model 用旧文本（不含粘贴内容）覆盖 textarea 中的粘贴内容。

## 修复

两处 paste 循环后添加 `model.input.document.clear()` + `insert_text()` 同步，让 InputModel 与 textarea 保持一致，后续 model→widget 推送不会丢失粘贴内容。

## 相关提交

- `29fe48f` fix: 修复粘贴后按空格清空粘贴内容 (refs #78)

## 验证

2026-05-30 用户确认 bug #78 已修复。
