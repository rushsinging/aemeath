# 权限 / Hook / 审计

**Scope**：`agent/features/{policy,hook,audit}/**`——权限评估、hook 执行、审计记录。
**主触发**：改 `agent/features/{policy,hook,audit}/**`。
**次触发**：改 hook 执行环境变量注入。

## Policy（权限）

- 权限 / 安全评估：`agent/features/policy/src/business/security.rs`。
- 完整权限管控系统（PermissionEngine、AskMe / Auto / Plan / AllowAll 模式、audit/policy 域）仍在设计/实施中，设计依据见 `docs/snapshot/active.md` 的 #42 与 `docs/snapshot/specs/042-permission-control-system.md`、`047-ddd-redesign.md`。改动前先核对该 feature 当前状态，**NEVER** 把尚未落地的设计当作既成约束。

## Hook

- Hook 稳定 PL 与能力矩阵：`agent/features/hook/src/domain/**`；`HookPort`：`agent/features/hook/src/ports.rs`。
- Hook 进程执行 adapter：`agent/features/hook/src/adapters/process.rs`；旧兼容 Runner：`agent/features/hook/src/adapters/legacy/**`。
- **Hook 执行环境 MUST** 同时注入 `AEMEATH_PROJECT_DIR` 与 `CLAUDE_PROJECT_DIR`（兼容投影在 `adapters/legacy/runner.rs`），兼容现有 Claude Code hook 脚本。
- Claude Code hooks 配置 → Aemeath hooks 的结构转换在配置层 `agent/shared/src/config/hooks.rs`，见 `config-compat.md`。

## Audit（审计）

- 审计域当前为骨架（`agent/features/audit/`）。
- 旧的 Agent 审计日志 `~/.agents/logs/agent.log` 已废弃（保留兼容枚举，当前无写入点）。
