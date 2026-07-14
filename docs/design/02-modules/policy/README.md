# Policy（支撑域）

> 层级：02-modules / policy（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> 本模块拥有 Tool 执行前的权限判断语言。本期只落地 AllowAll；其余决策只保留 Published Language 扩展点。

## 1. 模块定位

Policy 回答“这次 Tool 调用在当前策略下是否允许”，但不执行 Tool、不运行 Hook、不向用户提问，也不编排 Run。

```text
Runtime Tool Coordination
  → PolicyPort.evaluate(request)
  → PolicyDecision
  → Runtime 决定下一步控制流
```

Policy 与路径安全、内容扫描可共享安全概念，但本期不将既有散点 guard 包装成完整规则引擎。

## 2. 本期能力边界

v0.1.0 只有：

```rust
enum PolicyMode {
    AllowAll,
}
```

- CLI `--yolo` 是入站 adapter 的命名，映射为领域语言 `PolicyMode::AllowAll`；
- `AllowAllPolicy` 对所有合法 PolicyRequest 返回 Allow；
- 不实现规则匹配、Deny、人工审批、审批持久化或 delegated approval；
- 不把“接口中存在某个变体”描述为“本期支持该行为”。

## 3. Published Language

```rust
trait PolicyPort: Send + Sync {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision;
}

struct PolicyRequest {
    run_id: RunId,
    run_step_id: RunStepId,
    tool_name: ToolName,
    required_capabilities: ToolCapabilities,
    workspace_root: WorkspaceRoot,
}

enum PolicyDecision {
    Allow,
    Deny { reason: PolicyReason },
    RequireApproval {
        reason: PolicyReason,
        subject: ApprovalSubject,
    },
}
```

`Deny` 与 `RequireApproval` 是兼容预留。本期生产 adapter 产生非 Allow 属于未支持状态，必须被测试和装配约束阻止。

PolicyRequest 的字段也是为三态接口冻结的最小评估上下文；AllowAllPolicy 明确忽略 `required_capabilities` 与 `workspace_root`，但调用方仍必须提供合法值，避免 Future 启用 Deny/RequireApproval 时修改端口形状。v0.1.0 消费方不得根据这两个字段推断本期已存在规则评估。

PolicyRequest 只携稳定 PL，不包含 RuntimeContext、Tool 实例、HookRunner、TUI channel 或具体配置对象。

## 4. Future 边界

未来实现 Deny / RequireApproval 时，职责仍按以下边界：

| 能力 | 所有者 |
|---|---|
| 规则、路径/能力约束、PolicyDecision | Policy |
| 何时评估、暂停/恢复 Run | Agent Runtime |
| 如何向用户展示和收集答案 | Runtime Interaction |
| Tool 函数调用 | Tool BC |
| Policy 事实审计 | Future Audit 扩展 |

本期不预设计规则优先级、scope refinement、always-allow 存储或审批 UI。

## 5. 相邻安全机制

以下能力不属于 v0.1.0 Policy Engine：

- 工作区路径规范化与边界校验；
- Bash / command safety；
- AGENTS.md / guidance 内容 warning；
- PermissionRequest Hook。

内容 warning 是非阻断 assessment，由 Context Management 决定如何展示或注入；它不是 Tool PolicyDecision。只有未来出现稳定共同不变量时，才考虑共享规则基础设施。实现差距统一记录在 Migration Governance。

## 6. 装配

Composition Root：

1. 从 ConfigSnapshot 读取 PolicyMode；
2. v0.1.0 只构造 AllowAllPolicy；
3. 以 `Arc<dyn PolicyPort>` 注入 RuntimeContext；
4. CLI `--yolo` 不得越过 Config/Composition 直接修改 Runtime 内部字段。

Sub Run 可继承或收缩父 Run 的 Policy，但本期只有 AllowAll，因此不产生放宽/收缩差异。

## 7. 不变量

- **MUST** Policy 只返回决策，不编排控制流。
- **MUST** v0.1.0 生产实现只返回 Allow。
- **MUST** `PolicyMode` 作为 Future 规则模式的强类型扩展点；新增变体前必须完成对应 PolicyDecision 生产语义设计。
- **MUST NOT** 将 `--yolo` 作为领域枚举名称。
- **MUST NOT** 让 Policy 执行 Hook、Tool 或用户交互。
- **MUST NOT** 把路径 helper 的存在描述为完整 Policy Engine。
- **MAY** Future 扩展 Deny / RequireApproval，但必须另行设计规则与 Approval Gate。

## 8. 相关文档

- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Runtime Loop：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- Hook 设计：[../hook/README.md](../hook/README.md)
- Migration：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：AllowAll-only 实现范围与三态 PolicyPort 扩展边界 | #790 |
