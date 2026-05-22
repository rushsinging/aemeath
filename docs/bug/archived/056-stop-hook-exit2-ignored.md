# Bug #56: Stop hook 返回 exit 2 后 LLM 仍结束

- **发现日期**：2026-05
- **归档日期**：2026-05-22
- **状态**：已确认修复
- **优先级**：高

## 症状

Stop hook 返回 exit 2 后，LLM 仍然结束对话，无法收到问题并继续修复。

## 根因

Stop hook 使用 `run_plain` 执行，忽略 blocked 结果；即使检查脚本 exit 2，也只在日志中记录，不会阻止 TUI agent loop 结束。

## 修复

修复 Stop hook 执行路径，正确处理 exit 2 阻止 agent loop 结束，将 hook 反馈注入消息让 LLM 继续修复。
