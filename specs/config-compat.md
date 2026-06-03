# 配置分层 / Claude Code 兼容

**Scope**：`agent/shared/src/config/**`——配置分层、provider 默认值（base URL / 默认 model / env 名）、Claude Code 兼容、运行时路径。
**主触发**：改 `agent/shared/src/config/**`。
**次触发**：新增 `AEMEATH_*` 配置项，或改指令 / 配置 / skills / hooks 的读取优先级。

## 配置分层（优先级从高到低）

1. CLI 参数（`--provider`、`--model` 等）
2. 环境变量（`AEMEATH_*`、`ANTHROPIC_API_KEY` 等）
3. 项目级配置：`.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json` 的 hooks 配置
4. 全局配置（`~/.agents/aemeath.json`）
5. 硬编码默认值

入口：`agent/shared/src/config.rs`；运行时路径解析：`agent/shared/src/config/paths.rs`。

## Provider 默认值

- 每个 provider 的默认 base URL、默认 model、API key 环境变量名定义在 `agent/shared/src/config/models/`（`types.rs`、`resolve.rs`）与 `agent/shared/src/config/legacy.rs`。
- **NEVER** 硬编码 API key、base URL；新增 provider 的默认值在此补充（实现层见 `provider.md`）。

## Claude Code 兼容

- 项目指令读取 **MUST** Claude 优先：`{cwd}/CLAUDE.md` 优先，其次 `{cwd}/AGENTS.md`；全局指令仍读取 `~/.agents/AGENTS.md`。
- 项目配置读取 **MUST** `.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json`；Claude Code hooks 结构需转换为 Aemeath hooks（转换逻辑在 `agent/shared/src/config/hooks.rs`）。
- 项目 skills 读取 **MUST** `.claude/skills` 优先，其次 `.agents/skills`；同名 skill 以 Claude Code 项目 skill 为准。
- Hook 执行环境的 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 注入在 hook 域，见 `policy-hook-audit.md`。
