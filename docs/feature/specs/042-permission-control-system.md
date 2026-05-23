# Feature #42：权限管控系统设计

## 背景

原始 #42 关注点是：Allow All 模式下，用户明确要求访问 workspace 外路径时，Glob/Grep 仍被 workspace 边界拦截。进一步讨论后，范围升级为完整权限管控系统设计：既要支持外部路径授权，也要把工具审批、路径边界、风险评估和权限模式统一到一套模型里。

本设计参考 superpowers 的 brainstorming 流程完成，并确认以下产品方向：

- 采用 B + C 组合方案。
- B 不做命令式 `/allow-path` 主入口，改为 TUI 交互式授权选择。
- C 作为内部统一权限评估模型。
- 权限模式包含 `AskMe`、`Auto`、`Plan`、`AllowAll`。
- `AllowAll` 保留 root / YOLO 语义，接近当前 allow all 行为。
- `Sandbox` 仅作为未来扩展预留，本轮不纳入实现。
- 不引入 WorkspaceOnly / NoNetwork / NoShell / ConfirmDestructive / NonInteractive 等复杂 modifier。

## 目标

1. 建立统一 `PermissionEngine`，所有工具调用先转换为 action / resource / risk / profile，再输出权限决策。
2. 支持交互式授权外部路径，授权可作用于本次或当前 session。
3. 明确定义四种权限模式：AskMe、Auto、Plan、AllowAll。
4. 让 AllowAll 成为真正的 root 权限模式，默认放行并记录审计，不再被 workspace 边界阻断。
5. 为后续 Bash 风险分类、Agent 权限继承、MCP 工具权限声明预留模型。

## 非目标

1. 本轮不实现 Sandbox 隔离执行模式。
2. 本轮不引入权限 modifier。
3. 本轮不把交互授权设计为 slash command 主入口。
4. 本轮不要求一次性接入所有工具；可以先接入 Read / Glob / Grep / Edit / Write，再逐步接 Bash / Agent / MCP。
5. 本轮不持久化外部路径授权到 global 配置；P0 仅支持 once / session 生命周期。

## 核心模型

### PermissionRequest

每次工具调用在执行前转换为统一请求：

```text
PermissionRequest
- actor: MainAgent | SubAgent | McpTool
- tool: Read | Glob | Grep | Edit | Write | Bash | Agent | WebFetch | ...
- action: PermissionAction
- resources: Vec<PermissionResource>
- risk: RiskLevel
- profile: PermissionMode
- context:
  - workspace_root
  - path_base
  - session_grants
  - parent_actor / parent_tool_call
```

### PermissionAction

建议动作枚举：

```text
ReadFile
SearchFiles
SearchContent
InspectCode
WriteFile
CreateFile
DeleteFile
ShellExecute
NetworkRead
NetworkWrite
Delegate
```

### PermissionResource

```text
Path(path)
Command(command, cwd)
Url(url)
Agent(role)
Unknown
```

路径资源必须 canonicalize 后再判断边界。不存在的新文件允许按父目录 canonicalize 后判断。

### PathScope

```text
Workspace
AuthorizedExternal
UnknownExternal
Sensitive
```

其中 `AuthorizedExternal` 由交互式授权生成，携带 capabilities。

### Capability

```text
Read
Write
Execute
Network
Delegate
```

### RiskLevel

```text
Safe
Low
Medium
High
Critical
```

P0 可先粗粒度实现：只读为 Safe/Low，workspace 写为 Medium，外部写和 Bash 为 High，敏感路径和明显危险命令为 Critical。

### PermissionDecision

```text
Allow { reason, audit }
Ask { reason, risk, options }
Deny { reason }
```

`Ask` 的 options 由权限引擎生成，TUI 只负责渲染和回传选择。

## 权限模式

### AskMe

保守交互模式。

- workspace 内只读：自动允许。
- workspace 内写入：询问。
- Bash：询问。
- 外部路径：询问。
- 高危操作：询问或拒绝。

适合敏感项目、新项目、调试权限系统。

### Auto

日常开发默认模式，高权限但有护栏。

- workspace 内读写：自动允许。
- workspace 内常规开发 Bash：自动允许。
- 已交互授权的外部 scope：按授权 capability 自动允许。
- 未授权外部路径：弹出授权选择。
- 高危操作：弹出确认或拒绝。

### Plan

只规划 / 只分析模式。

- 允许 Read / Glob / Grep / LSP / 少量只读命令。
- 不允许写文件。
- 不允许执行有副作用的 Bash。
- 不允许提交、删除、构建产物变更。
- 外部路径仍通过交互式授权控制。

适合需求分析、方案设计、bug 根因调查、code review。

### AllowAll

root / YOLO 模式，接近当前 allow all。

- 默认允许 workspace 内外读写执行。
- 默认允许 Bash。
- 默认不弹确认。
- 权限系统仅做结构化审计记录。
- 不因 workspace 边界拒绝外部路径。

## 交互式授权

当 `PermissionDecision::Ask` 需要用户选择时，TUI 展示系统级权限 prompt，而不是伪装成普通 LLM 工具。

示例：

```text
需要授权外部路径访问

工具：Grep
动作：搜索内容
路径：/Users/me/other-project
风险：Medium
原因：该路径位于当前 workspace 外，尚未授权。

请选择：
- 允许本次
- 本 session 允许只读访问该目录
- 本 session 允许读写访问该目录
- 本 session 允许读写执行该目录
- 拒绝
```

用户选择后生成：

```text
PermissionGrant
- scope: canonicalized directory
- capabilities: Read | Write | Execute
- lifetime: Once | Session
- source: UserInteractive
```

`AllowAll` 下不弹出交互式授权，直接允许并审计。

## P0 实现建议

1. 在 `aemeath-core` 中新增权限模型：`PermissionMode`、`PermissionRequest`、`PermissionAction`、`PermissionResource`、`RiskLevel`、`PermissionDecision`、`PermissionGrant`。
2. 引入 `PermissionEngine::evaluate(request, context)`。
3. 将现有 `PermissionMode::{Ask, AutoRead, AllowAll}` 迁移或兼容到 `AskMe / Auto / Plan / AllowAll`。
4. 在 `ToolContext` 增加 session grants，保存 canonicalized 外部目录授权。
5. 统一路径边界判断：workspace 内允许；外部路径按 grants 和当前 profile 评估；AllowAll 默认允许。
6. 先接入 Read / Glob / Grep，解决 #42 原始场景。
7. 再接入 Edit / Write，确保外部写权限必须有 Write capability，AllowAll 除外。
8. TUI 增加权限 prompt，支持 once / session 授权选择。
9. 更新审计日志或普通日志，记录 tool、action、resource、risk、decision。

## P1 / P2 方向

### Bash

- 将 Bash 命令分类为只读、常规开发命令、高危命令。
- Auto 下常规开发命令自动允许，高危命令询问或拒绝。
- AllowAll 下默认允许。

### Agent / Sub-Agent

- 子代理权限默认不得超过父 agent 当前 profile。
- AllowAll 可显式继承 root 权限，但需要审计。
- Agent task 可传递 capability subset，便于后续细粒度控制。

### MCP

- MCP 工具应声明 action / resource / risk。
- 未声明风险的 MCP 工具默认 AskMe / Auto 下询问，AllowAll 下允许。

### Sandbox

未来可新增 Sandbox 模式：写入和命令执行发生在隔离 worktree 或临时目录中，再由用户选择是否合并结果。本轮仅在模型中预留，不实现。

## 验收标准

1. AskMe / Auto / Plan / AllowAll 四种模式语义在代码和配置中清晰表达。
2. AllowAll 下 Glob/Grep/Read 外部路径不再因 workspace 边界被拒绝。
3. Auto / AskMe / Plan 下，未授权外部路径触发交互式授权选择。
4. session 授权后，同一外部目录内对应 capability 的后续操作可自动通过。
5. 外部授权路径使用 canonicalize，不能通过 `..` 或 symlink 绕过边界。
6. Read / Glob / Grep 至少覆盖 workspace 内、外部未授权、外部已授权、AllowAll 外部路径四类测试。
7. 权限决策有审计记录，能区分 Allow / Ask / Deny / AllowAll。
