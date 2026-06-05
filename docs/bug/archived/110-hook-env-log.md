# Bug #110：Stop hook 项目上下文只输出到 stdout，成功时不进入 aemeath.log

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 569e5fc |

## 症状

Stop hook 脚本已输出 `[hook-env] AEMEATH_PROJECT_DIR=...`、`CLAUDE_PROJECT_DIR=...`、`ROOT/PWD=...`，但 hook 成功通过时，这些内容只出现在 hook stdout/TUI 验证输出中，不进入 `~/.agents/logs/aemeath.log`；即使将 `logging.level` 调为 `debug`，日志中也只能看到已有 hook start/end 元信息，无法直接检索 `[hook-env]` 行。

## 根因

`HookRunner::execute_hook` 成功等待子进程后只记录 stdout/stderr 字节数，没有记录 stdout/stderr 内容。为避免完整 hook 输出污染日志，需要只提取稳定的 `[hook-env]` 诊断行写入日志。

## 修复

1. 新增 `hook_env_lines`，从 stdout/stderr 中提取以 `[hook-env]` 开头的行。
2. `execute_hook` 在判定 blocked 前，将 stdout/stderr 中的 `[hook-env]` 行写入 `log::info!`，日志包含 event、command、stream 与 line。
3. 不记录完整 hook stdout/stderr，避免单测和构建输出大量进入主日志。

## 验证

- `cargo test -p hook test_hook_env_lines_extracts_only_hook_env_stdout_lines`
- 用户确认修复。

## 涉及路径

- `agent/features/hook/src/business/hook/runner.rs`
- `agent/features/hook/src/business/hook/tests.rs`

## 关联提交

- `569e5fc fix(hook): 记录 hook-env 诊断行 (refs #110)`
