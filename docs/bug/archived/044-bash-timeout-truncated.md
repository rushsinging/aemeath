# Bug #44: Bash 工具设置 600s timeout 仍被 120s 截断

- **发现日期**：2026-05
- **归档日期**：2026-05-19
- **状态**：已确认修复
- **优先级**：中

## 修复

BashTool 覆写 timeout_secs() 返回 600s，匹配 schema 最大允许值；agent.rs 外层超时不再在 Bash 内部 timeout 前截断 (355aca6)。
