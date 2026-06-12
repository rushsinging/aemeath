<!-- Migrated from: docs/feature/archived/040-claude-compatible-agents-config.md -->
# Feature #40：配置文件改造：对齐 Claude 优先兼容的 `~/.agents` / `CLAUDE.md` / skills 读取

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

将 Aemeath 的全局配置、项目配置、项目指令、skills、guidance、memory、sessions、history、cost history、mcp、settings、logs 等路径统一迁移到 `~/.agents` 体系，同时保留 Claude Code 兼容读取能力。

## 实现结果

1. 全局配置根默认迁移到 `~/.agents` 且可配置。
2. agent 配置文件使用 `aemeath.json`。
3. 项目指令 Claude 优先读取 `{cwd}/CLAUDE.md`，不存在时 fallback 到 `{cwd}/AGENTS.md`。
4. 全局指令读取 `~/.agents/AGENTS.md`。
5. 项目配置优先级为 `{cwd}/.agents/aemeath.json` > `{cwd}/.claude/settings.json` > 全局 `~/.agents/aemeath.json`。
6. Claude Code hooks 结构转换为 Aemeath hooks。
7. 项目 skills 优先 `{cwd}/.claude/skills`，其次 `{cwd}/.agents/skills`，全局 `~/.agents/skills` 作为 fallback。
8. guidance、memory、sessions、history、cost_history、mcp、settings、logs 等运行数据迁移到 `~/.agents`。
9. logging 改为单一全局 `level`，不再支持按模块配置；旧 `default_level` 兼容读取，旧 `module_levels` 忽略。

## 验证

2026-05-24 用户确认 feature #40 已完成。活动列表中移除 #40，并保留此归档记录。
