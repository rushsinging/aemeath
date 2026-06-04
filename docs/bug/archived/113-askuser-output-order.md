# Bug #113：AskUserQuestion 回答后 LLM 新输出渲染到 AskUser 块上方；AskUser 块本身固定在初始位置

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 3f13d7d, 1ab5d01 |

## 症状

**现象 1**：用户回答 AskUserQuestion 后，LLM 继续生成的文本出现在 AskUser 交互块的上方而非下方。

**现象 2**：AskUser 块本身不会随着新输出推进而向下滚动，而是固定在初始渲染位置；LLM 后续文本、工具调用结果等增量内容都在 AskUser 块上方区域不断刷新，视觉上 AskUser 像被"钉"在屏幕某一位置。

## 根因

`append_assistant_text` 通过 `append_or_extend_text_block` 追加文本：如果 `active_text_block_id` 仍指向 AskUser 之前的 AssistantText 块，新文本会被 extend 到该旧块（位于 AskUser 块之前）。

`active_text_block_id` 只在 `TextBlockComplete`（`model.rs:262`）和 `complete_chat`（`model.rs:227`）时清除。AskUserQuestion 工具调用前如果没有收到 `TextBlockComplete` 事件，`active_text_block_id` 就不会变为 None，后续文本就会 extend 到旧块。

现象 1 与现象 2 同根因：因为新文本都被错误地 extend 到 AskUser 之前的旧 AssistantText 块，AskUser 块的相对位置就不会被新内容推动向下；视觉表现为 AskUser 固定不动，其上方的旧文本块不断扩张。

## 修复

在 AskUser 显示和回答时清理 ConversationModel 的活跃文本块边界（清空 `active_text_block_id`），强制后续 assistant text 创建新 AssistantText 块。新块自然位于 AskUser 块之后，从而在 AskUser 下方追加。

## 验证

- `cargo test -p cli ask_user`
- `cargo test -p cli conversation`
- `cargo check -p cli`
- 新增回归测试覆盖 `StartChat → assistant streaming → AskUser → answer → assistant streaming` 的顺序，确保新输出创建新 AssistantText 并位于 AskUser 下方
- 用户确认修复。

## 涉及路径

- `apps/cli/src/tui/model/conversation/ask_user.rs`
- `apps/cli/src/tui/model/conversation/model.rs`

## 关联提交

- `3f13d7d fix(tui): AskUser 后续输出保持在块下方 (refs #113)`
- `1ab5d01 merge: bug/113-askuser-output-order — 修复 AskUser 后续输出顺序 (refs #113)`
- `21da304 docs(bug): 补充 #113 现象 2 AskUser 块固定不滚动 (refs #113)`
- `ff46821 fix(prompt): 隔离 guidance 初始化单测环境 (refs #113)`
- `4cffc6c docs: 登记 Bug #113 (refs #113)`
