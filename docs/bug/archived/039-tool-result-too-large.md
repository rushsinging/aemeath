# Bug #39: 超大工具结果触发 API 400 string_above_max_length

- **发现日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认修复
- **优先级**：高

## 症状

工具返回超大结果时，完整内容拼入 LLM 请求体，触发 API 400 string_above_max_length 错误。

## 修复

TUI 主 loop 与子 Agent loop 在工具结果进入 LLM 前持久化超大输出，截断后再送入请求。
