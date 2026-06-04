# Feature #73：AGENTS.md 渐进式披露重构

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 登记日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认完成 |

## 背景

根 `AGENTS.md`（`CLAUDE.md` 软链指向它）旧版是单文件全量约 185 行，无论当前干什么活都全量加载。参考 `wanaka-platform` 的 Progressive Disclosure 做法，根文件只保留全局始终适用的「宪法 + 工作流 + 路由表」，detailed 规则按工作范围下沉到 `specs/` 分片，按需加载。

**附带漂移修正**：旧 AGENTS.md 大量引用 `aemeath-core/`、`aemeath-llm/`、`aemeath-tools` 等已不存在的旧 crate 名，本次重构同步校正为真实的 `agent/features/*` + `agent/shared/*` 路径。

## 实现

1. 根 `AGENTS.md` 瘦身为：标题 + Constitution + 项目结构 + 运行时目录 + 工作流约束 + 渐进式披露段（含路由表）+ 开放决策
2. 新建 `specs/` 10 个分片，每个 spec 顶部带 scope 声明：
   - `rust-coding.md`（横切 `**/*.rs`：编码/测试/日志/验证门禁/错误处理）
   - `tui-cli.md`（`apps/cli/src/**`：TUI/REPL）
   - `runtime.md`（`agent/features/runtime/**`：Agent 循环、tool 编排、token budget、compact、pricing、slash 命令）
   - `tools.md`（`agent/features/tools/**`：Tool trait、ToolRegistry、MCP）
   - `provider.md`（`agent/features/provider/**`：provider HTTP/stream 实现）
   - `prompt.md`（`agent/features/prompt/**`：Guidance、系统提示、上下文注入）
   - `config-compat.md`（`agent/shared/src/config/**`：配置分层、provider 默认值、Claude Code 兼容、paths）
   - `storage.md`（`agent/features/storage/**`：memory、task、history、tool_result）
   - `policy-hook-audit.md`（`agent/features/{policy,hook,audit}/**`：权限、hook 执行环境、审计）
   - `bug-feature-tracking.md`（无路径触发：docs/ 编号/状态/归档门禁）
3. 「命令系统」并入 `runtime.md`
4. 顺带修复 EnterWorktree：支持目标不存在时基于 main 自动创建（commits `3bb1308 / 29e3654`）

## 验收

- 根 `AGENTS.md` 不再包含 detailed 规则，行数显著下降
- `specs/` 下 10 个分片齐全，每个 spec 顶部有 scope 声明
- 路由表每行的主触发路径在仓库中真实存在
- 现有规则在新结构中均有对应落点，无规则丢失
- 用户确认完成。
