# Bug #49: last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域

- **发现日期**：2026-05
- **归档日期**：2026-05-22
- **状态**：已确认修复
- **优先级**：高

## 症状

用户在 LLM 处理期间提交的消息（last turn）不会被发送给 LLM，留在 input queue 区域。

## 根因

`process_in_background` 中 `tool_calls.is_empty() || stop_reason==EndTurn` 时直接 `break` 退出 loop，未消费 `input_queue` 中用户排队消息。工具轮结束后即使消费了队列也未立即 `continue`，可能先执行收尾逻辑。

## 修复

抽取 `append_queued_input`，在 EndTurn/无工具调用和工具轮结果同步后统一 drain queued input，有消息则同步 messages 并 continue 进入下一轮。补充正常/空队列/通道关闭单元测试。
