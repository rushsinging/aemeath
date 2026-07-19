# Policy（支撑域）

> 层级：02-modules / policy（模块战术设计）
> 状态：Target｜Milestone：v0.1.0｜对应 Issue：#1221 / #1229–#1232

## 1. 模块定位

Policy 拥有 Tool invocation 的唯一授权语言。Runtime 对每个 ToolCall 调用一次 `PolicyPort::evaluate`，再把 Policy 返回的 `AuthorizationContext` 原样投影给 Tool、Project 与授权性 Hook/fuse；这些消费者 **NEVER** 自行读取 Config 或 `allow_all`。

## 2. Config 驱动模式

```text
PermissionModeConfig::Ask | AutoRead -> PolicyMode::Standard
PermissionModeConfig::AllowAll       -> PolicyMode::AllowAll
```

v0.1.0 尚无审批状态机，因此 Ask/AutoRead 暂时映射为 Standard：保留 workspace containment、read-before-write、Bash safety、Tool fuse 与 permission hooks，但不伪造审批。

`ConfiguredPolicy` 每次 evaluate 都经 `PolicyModeSource` 读取 committed ConfigSnapshot，动态 permission update 对下一次 ToolCall 立即生效。Config 固定优先级为 `CLI > Env > Local config > Global config > Default`：CLI/Env 是启动期永久覆盖，动态 update 属 Local 层，仅在上层未指定时生效。CLI `--yolo` / `--allow-all` 不向 Runtime/Tool 传播第二个业务 bool。

## 3. Published Language

```text
PolicyRequest { run_id, run_step_id, tool_name, required_capabilities, workspace_root }
PolicyDecision::Allow(AuthorizationContext)
PolicyDecision::Deny { reason }                  # Future
PolicyDecision::RequireApproval { reason, subject } # Future
```

`AuthorizationContext` 的唯一类型定义归 Tools Published Language，因为 Tool adapters 必须消费它且 Tools 不反向依赖 Policy。Policy 负责构造，Runtime 负责逐调用传递，Tools 只读消费。

Standard：

- `allow_outside_workspace = false`
- `require_read_before_write = true`
- `enforce_bash_safety = true`
- `enforce_tool_fuse = true`
- `enforce_permission_hooks = true`

AllowAll：上述授权性限制全部反转；不设置敏感路径 hard deny 或白名单。仅保留 schema/参数、Tool 注册与 capability 元数据、文件存在性、OS 权限、I/O、取消和超时等客观错误。

## 4. 边界

- Project 只提供 lexical normalize、canonicalize、symlink resolution 与显式授权参数下的路径解析，不读取 Config/Policy。
- Tool adapter 消费逐调用 AuthorizationContext，不读取 `allow_all`。
- Runtime 必须在 Main/Sub/MCP 统一调用 Policy，并在 Tool fuse/permission hooks 前保留授权上下文。
- Policy 不执行 Tool、Hook、用户交互或 Runtime 控制流。
- Guidance 内容 warning 仍属于 Context assessment，不受 AllowAll 影响。

## 5. 不变量

- **MUST** Config committed permission mode 是唯一模式真相。
- **MUST** 每个 ToolCall 有且仅有一个 PolicyDecision 和一个 AuthorizationContext。
- **MUST** AllowAll 无条件放行所有授权性限制。
- **MUST** Main/Sub、内置/MCP 使用同一 PolicyPort。
- **NEVER** 在 Project/Tool/Runtime/Hook 再读取业务 `allow_all`。
- **NEVER** 恢复 Runtime PermissionMode、Tools PolicyDecision 或 Guidance::allow_all 双轨。

## 6. Target 目录

Policy 保持单能力扁平 `domain.rs + adapters.rs`：domain 定义 Request/Decision/Mode/Port，adapters 实现 Standard、AllowAll 与 ConfiguredPolicy。没有独立规则引擎或审批用例前，**NEVER** 预建 application/ports/capabilities。

## 7. 验证

- L1：Config mode 映射和五维授权矩阵。
- L2：路径、read-before-write、Bash、fuse、permission hooks。
- L3：动态 Config 更新、Main/Sub/MCP 同一授权契约。
- L4：CLI/config AllowAll 读取项目外 hook 结果并执行原 safety 会拒绝的操作。
- L0：守卫禁止重复权限类型、Tool-local allow_all 与 Project 自主授权。
