# 权限 / Hook / 审计

**Scope**：`agent/features/{policy,hook,audit}/**`——权限评估、hook 执行、审计记录。
**主触发**：改 `agent/features/{policy,hook,audit}/**`。
**次触发**：改 hook 执行环境变量注入。

## Policy（权限）

- `PolicyPort`、`PolicyRequest/Decision/Mode` 位于 `agent/features/policy/src/domain.rs`；`ConfiguredPolicy` 位于 `adapters.rs`。
- Config committed permission mode 是唯一模式真相；CLI yolo 只作为 Config override。
- `AllowAll` **MUST** 放行所有授权性限制：项目外路径、read-before-write、Bash safety、Tool fuse 与 permission hooks；**NEVER** 增加敏感路径 hard deny。
- `AuthorizationContext` 唯一定义在 Tools Published Language；Policy 构造、Runtime 逐调用传递、Tool/Project/Hook 只读消费。
- **NEVER** 在 Runtime/Project/Tool/Hook 直接读取业务 `allow_all`，或恢复重复 PermissionMode/PolicyDecision。
- Deny/RequireApproval 仍为 Future PL；v0.1.0 Ask/AutoRead 映射 Standard 约束，不伪造审批。

## Hook

- Hook 稳定 PL 与能力矩阵：`agent/features/hook/src/domain/**`；`HookPort`：`agent/features/hook/src/ports.rs`。
- Hook 进程执行 adapter：`agent/features/hook/src/adapters/process.rs`；旧兼容 Runner：`agent/features/hook/src/adapters/legacy/**`。
- **Hook 执行环境 MUST** 同时注入 `AEMEATH_PROJECT_DIR` 与 `CLAUDE_PROJECT_DIR`（兼容投影在 `adapters/legacy/runner.rs`），兼容现有 Claude Code hook 脚本。
- Claude Code hooks 配置 → Aemeath hooks 的结构转换在配置层 `agent/shared/src/config/hooks.rs`，见 `config-compat.md`。

## Audit（审计）

- 审计域当前为骨架（`agent/features/audit/`）。
- 旧的 Agent 审计日志 `~/.agents/logs/agent.log` 已废弃（保留兼容枚举，当前无写入点）。
