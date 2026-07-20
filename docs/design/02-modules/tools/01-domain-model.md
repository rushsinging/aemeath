# Tool & Skill & Command · 领域模型

> 层级：02-modules / tools（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#787（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文只描述目标态；当前实现证据与差距统一记录在 [Migration Governance](../../03-engineering/03-migration-governance.md)。

## 1. Tool 语言

### 1.1 ToolDescriptor

```rust
struct ToolDescriptor {
    name: ToolName,
    description: LocalizedText,
    input_schema: JsonSchema,
    required_capabilities: ToolCapabilities,
    required_resources: ResourceRequirements,
    concurrency: ConcurrencyDeclaration,
    cancellation: CancellationDeclaration,
}

struct ConcurrencyDeclaration {
    safety: ConcurrencySafety,       // Safe | Serialized
}

enum CancellationDeclaration {
    Cooperative,
    NonCooperative,
}
```

ToolDescriptor 是 Tool Catalog 的 Published Language，不包含 Tool 实例、来源 adapter、MCP server、函数指针、transport、client 或 Registry 引用。`ConcurrencySafety::Safe` 允许 Runtime 与其他安全 Tool 并发；`Serialized` 要求 Runtime 对同一 ToolName 串行调度。`CancellationDeclaration` 只声明协作能力，不携带 timeout 策略。

| Concurrency | Cancellation | Runtime 规约 |
|---|---|---|
| Safe | Cooperative | 可与其他 Safe Tool 并发；timeout 后请求取消并等待受控清理 |
| Safe | NonCooperative | 可并发，但 timeout 后可能继续副作用；必须提示风险并限制同名重入 |
| Serialized | Cooperative | 同一 ToolName 全局串行；timeout 后取消完成才能放行下一次 |
| Serialized | NonCooperative | 同一 ToolName 串行且 timeout 后保持占位直到底层结束；不得启动第二次调用 |

`Serialized` 的序列化键是 ToolName；不同 Serialized Tool 可并发。Runtime 负责调度与 timeout，Tool BC 只发布声明并响应 CancellationSignal。

`ToolName` 是本阶段的规范化逻辑键：名称在同一 Registry Scope 内唯一，重名注册失败，禁止隐式覆盖。稳定 ID、版本与重命名兼容属于 MCP 动态接线阶段的独立决策。

### 1.2 ToolCapabilities

Tool 声明执行所需能力；Profile 声明允许能力。初始能力词汇：

```rust
enum ToolCapability {
    ReadWorkspace,
    WriteWorkspace,
    ExecuteProcess,
    NetworkAccess,
    UserInteraction,
    AgentDispatch,
    TaskRead,
    TaskMutation,
    WorkspaceControl,
    PlanControl,
}
```

Capability 表达安全权限，不表达 Tool 身份或装配位置。新增 Tool 未声明 required capabilities 时不得注册。

```rust
/// ToolCapabilities 是 ToolCapability 的 bitflag / HashSet 容器。
type ToolCapabilities = Vec<ToolCapability>;
```

### 1.3 ResourceRequirements

Tool 所需活资源通过窄端口提供，例如：

- Project-owned `WorkspaceRead`
- Project-owned `WorkspaceControl`
- `FileAccess`
- `TaskAccess`
- `AgentDispatch`

Descriptor 声明 required resources，Registry Scope 装配时验证资源齐备。资源端口不等同 capability：capability 决定“能否授权”，resource 决定“是否有实现可用”。Tool BC **MUST** 直接消费 Project 发布的窄 trait，**NEVER** 再定义或预装配覆盖读写控制的通用 Workspace wrapper。

| Tool 类别 | Project 能力 | 约束 |
|---|---|---|
| 文件 Tool（Read / Write / Edit / Glob / Grep） | `WorkspaceRead` | 用于路径解析；其中只读文件 Tool **NEVER** 获得 Control |
| Bash | `WorkspaceRead` + `WorkspaceControl` | **MUST** 仅在同步 `cd` / path base 时使用 Control |
| EnterWorktree / ExitWorktree | `WorkspaceRead` + `WorkspaceControl` | 由 Project 守护状态转换，并在转换后从同一 wiring 的 Read 获取 path / root / branch 生成 Tool 结果 |
| AskUser | 无活资源 | 只解析为 `ToolSuspension::UserInteraction`；交互、等待与 Run 恢复归 Runtime |
| 其他 Tool | 按 descriptor 声明 | 未声明 workspace resource 时 **NEVER** 注入任一 Project 能力 |

`WorkspaceControl` 的 resource 与同名 `ToolCapability::WorkspaceControl` 权限 **MUST** 同时满足：前者证明实现已装配，后者证明调用被授权。只有 Bash、EnterWorktree、ExitWorktree **MAY** 声明该 resource；增加第四个消费者 **MUST** 先修改 Project 消费方契约与架构测试。

## 2. Registry Scope 与 Tool Profile

### 2.1 Registry Scope

Registry Scope 是一次 RuntimeContext 装配出的 Tool 实例与资源集合，回答“这次 Run 实际有什么”。例如：

- Main Scope：可装配顶层用户交互、共享 Task、Workspace、Agent Dispatch；
- Sub Scope：按 RunSpec 只装配允许继承或隔离后的资源；
- 其他 Scope 必须按目的命名，不使用含义模糊的 `NoAgent`；迁移期历史 `NoAgent` 集合命名为 `legacy-no-agent`，并由 #914 退役。

### 2.2 Tool Profile

Tool Profile 是能力允许集合，回答“已装配能力中允许用什么”：

```rust
struct ToolProfile {
    allowed_capabilities: ToolCapabilities, // 私有字段
}

impl ToolProfile {
    fn baseline(allowed: ToolCapabilities) -> Self;

    fn derive_restricted(
        parent: &ToolProfile,
        requested: ToolCapabilities,
    ) -> Result<Self, ProfileExpansionError>;
}

enum ProfileExpansionError {
    CapabilityExpansion { capabilities: ToolCapabilities },
}
```

`derive_restricted` 只在 `requested ⊆ parent.allowed_capabilities` 时构造子 Profile；字段私有且不提供扩大 capability 的 mutation API。

有效工具集：

```text
Effective Tools = Registry Scope ∩ Profile Allowed Capabilities
```

一个 Tool 必须依次通过三阶段校验：

1. **Scope 装配阶段**：ToolName 在 Scope 内唯一、required resources 齐备、schema 可解析；失败则拒绝装配。
2. **Catalog 投影阶段**：Scope 已装配 Tool，且 required capabilities 全部包含于 Profile allowed capabilities；失败则不进入 Snapshot。
3. **Execution 阶段**：调用瞬间重新检查 Tool 仍存在于 Scope、Profile 仍允许、resources 仍可用，并用当前 schema 校验 input；失败返回 ToolOutcome::Failure。

三阶段职责不同，Catalog 通过不能替代 Execution 的最后检查。

### 2.3 只收缩不变量

- Profile 派生必须调用 `derive_restricted(parent, requested)`；若 `requested.allowed_capabilities` 不是 parent 的子集则拒绝构造；
- ToolProfile 字段私有，不提供直接扩展 capability 的 mutation API；
- Profile 只能过滤 Scope，不能向 Scope 添加 Tool 或 Resource；
- Catalog 和 Execution 必须各自检查 Scope/Profile，不能只在注册时检查；
- 不使用 ToolName 黑名单表达权限；
- MCP annotations 缺失或不可信时使用保守 capability，不得默认只读。

## 3. Tool Catalog

```rust
struct ToolCatalogSnapshot {
    scope: RegistryScopeName,
    profile: ToolProfileName,
    tools: Vec<ToolDescriptor>,
}
```

Snapshot 是只读投影。Tool Catalog 内部可由 built-in 与 MCP 来源组合，但消费者只看到统一 Descriptor。`tools` 顺序在 snapshot 生命周期内稳定；Tool BC 发布唯一的纯 `model_schemas()` 投影，把每个 descriptor 映射为 `ModelToolSchema` 且保持该顺序。Runtime 每次 invocation 只调用一次该投影，Context / Provider 不再从 descriptor 重建第二份 schema。

Catalog 变化发布以下生命周期事件；本阶段只承诺重新拉取语义，不定义 revision：

```rust
struct CatalogChanged {
    reason: CatalogChangeReason,
}

enum CatalogChangeReason {
    ScopeReassembled,
    ProfileChanged,
    ToolRegistered,
    ToolRemoved,
    ExternalSourceChanged,
}
```

CatalogChanged 不携带内部 Registry、MCP server 或 Tool 实例；消费者收到任何 reason 都重新拉取所需 Scope/Profile Snapshot。

## 4. Tool Invocation 与 Outcome

### 4.1 ToolInvocation

```rust
struct ToolInvocation {
    tool_name: ToolName,
    input: JsonValue,
    execution_scope: ExecutionScope,
}
```

Invocation 不携带 RuntimeContext、Registry、Session、Store 或 MCP 类型。

### 4.2 ToolOutcome

```rust
enum ToolOutcome {
    Success(ToolSuccess),
    Failure(ToolFailure),
    Cancelled(ToolCancelled),
    Suspended(ToolSuspension),
}

enum ToolSuspension {
    UserInteraction(UserInteractionSpec),
}

struct UserInteractionSpec {
    questions: Vec<UserQuestion>,
}

struct UserQuestion {
    prompt: String,                 // 向用户展示的问题文本
    options: Vec<String>,           // 可选选项；空 = 自由文本回答
    allow_multi: bool,              // 是否允许多选
}

struct ToolSuccess {
    content: Vec<ContentBlock>,
    data: Option<JsonValue>,
    metadata: ToolExecutionMetadata,
}

struct ToolFailure {
    kind: ToolErrorKind,
    safe_message: String,
    retryable: bool,
    content: Vec<ContentBlock>,
    data: Option<JsonValue>,
}

enum ToolErrorKind {
    ToolUnavailable,                // 工具未注册或已下线
    InvalidInput,                   // schema 校验失败
    PermissionDenied,               // 能力不足 / 权限拒绝
    Internal,                       // 工具内部执行错误
    Timeout,                        // 执行超时
    Cancelled,                      // 被取消
    Unsupported,                    // 平台 / 架构不支持
}
```

ToolOutcome 是领域结果，不依赖 SDK/TUI View。错误只公开可安全暴露的信息，不泄漏密钥、完整进程环境或 adapter 私有协议内容。`Suspended` 只是 Tool 对“完成该调用前需要外部回答”的 typed 表达；Tool BC **NEVER** 等待 UI、持有 reply channel 或改变 Run 状态。Runtime 将 reply 映射为同一 ToolCall 的最终 `ToolSuccess` 或 cancellation。

Tool BC 不负责 token budget、截断、超大结果持久化、Context Window 格式或 TUI 渲染。这些由 Runtime Tool Coordination、Context Management 与 Storage 协作。

## 5. Skill 模型

```rust
struct SkillDescriptor {
    name: SkillName,
    description: LocalizedText,
    source: SkillSource,
}

struct PromptFragment {
    stable_key: PromptFragmentKey,
    content: String,
    source: PromptFragmentSource,
    cache_hint: CacheHint,
}
```

Skill Catalog 负责发现；Skill Materialization 负责加载、解析并产出 PromptFragment。Skill 不作为 Tool，不执行函数，也不直接修改 System Prompt。

PromptFragment 是 Skill 与 Context Management 的 Published Language。Context Management 独占注入时机、位置、预算、去重、缓存分段及内容顺序。

## 6. Slash Command 模型

### 6.1 CommandDescriptor

```rust
struct CommandDescriptor {
    name: CommandName,
    aliases: Vec<CommandName>,
    description: LocalizedText,
    mechanism: CommandMechanism,
    argument_schema: CommandArgumentSchema,
}
```

Descriptor 支持发现、帮助和参数补全；补全基于 argument_schema，不执行命令。

```rust
struct CommandCompletion {
    replacement: String,
    display: String,
    description: LocalizedText,
}

enum SnapshotQueryTarget {
    Runtime,
    ContextManagement,
    Memory,
    Task,
    Project,
    Provider,
    Config,
    Audit,
    ApplicationShell,
}

// ApplicationShell 只标识 CLI/TUI/未来 Server 自身拥有的只读交付能力，
// 例如 help、pending images、version 与 doctor；它不得被用来包装目标 BC 业务查询。

enum ApplicationControlTarget {
    Runtime,
    ContextManagement,
    Memory,
    Task,
    Project,
    Config,
    ApplicationVersionControl,
    ApplicationShell,
}

struct PromptCommand {
    command: CommandName,
    arguments: ParsedArguments,
}

struct SnapshotQueryCommand {
    command: CommandName,
    arguments: ParsedArguments,
}

struct ApplicationControlCommand {
    command: CommandName,
    arguments: ParsedArguments,
}
```

Target 是封闭枚举；新增目标 BC 需扩展 Published Language 与路由测试。三种 command 是解析后的类型化请求，不承载目标 BC 的结果，也不退化成原始 Slash 字符串或无约束 JsonValue。

### 6.2 执行机制

Slash Command 共享 parser 和 router，但按执行机制分为三类：

```rust
enum CommandMechanism {
    PromptInjection,
    SnapshotQuery,
    ApplicationControl,
}
```

### PromptInjection

将命令参数转换为 PromptFragment，交给 Context Management，并通过正常 Run 执行。适合主要表达模型任务或提示模板的命令。

### SnapshotQuery

调用目标 BC 的只读 Query Port，返回该 BC 的 Published Snapshot；不创建 Run，也不直接读取 Runtime 内部结构。

### ApplicationControl

调用目标 BC 的应用 Command Port 执行状态变更，例如 compact、cancel、resume、model/thinking 配置修改、memory mutation 或 session deletion。`/thinking` 写入 Config；Workflow 只经 ReasoningPort 消费配置并调节 effort，不作为 Command target。

三类机制不共享执行 trait 或结果数据模型。Command Router 只负责解析为带类型的 route：PromptCommand、SnapshotQueryCommand 或 ApplicationControlCommand；每种 route 由对应 handler 调用目标端口。CLI、TUI、Server 使用各自 ACL 展示类型化结果。

## 7. MCP Connection 聚合

MCP 是 Tool adapter。目标态由 `McpConnection` 聚合守护连接生命周期：

```rust
enum McpConnectionState {
    Disabled,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
    Stopping,
}
```

不变量：

1. 只有 Connected 状态可向 Tool Catalog 发布 MCP Tool；
2. 连接状态与已发布 Tool 投影必须一致；
3. 状态迁移必须经聚合方法，禁止散点直接改写；
4. disconnect / failure 必须撤销对应 Catalog 投影；
5. MCP schema、annotations 与结果必须经 ACL 转成 Tool Published Language；
6. MCP transport、JSON-RPC 和认证 DTO 不得越过 adapter 边界。

本期只锁定 MCP 目标边界；health check、hot reload、resources、稳定 Tool ID 与 revision 的接线和兼容语义延后。

## 8. 聚合与服务边界

| 对象 | 类型 | 所有权 / 说明 |
|---|---|---|
| Tool Catalog | 领域服务 / 只读投影 | 组合 Scope、Profile 与来源 adapter |
| Tool Executor | 应用服务 | 校验局部不变量并调用一个 Tool |
| McpConnection | 聚合 | 守护单个 MCP server 连接生命周期 |
| Skill Catalog | 领域服务 | 发现 SkillDescriptor |
| Skill Materializer | 应用服务 | Skill → PromptFragment |
| Command Parser / Router | 领域服务 / 应用服务 | 解析并路由三种机制 |

## 9. 相关文档

- 模块入口：[README.md](README.md)
- 端口与生命周期：[02-ports-and-lifecycle.md](02-ports-and-lifecycle.md)
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Project Workspace 端口：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Tool PL、Scope/Profile、Outcome、Skill/Command 机制与 MCP 聚合 | #787 |
| 2026-07-14 | 移除通用 Workspace resource 包装，改为 Tool 按需直接消费 WorkspaceRead / WorkspaceControl，并将 Control 限于三个 Tool | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #993 过渡目录迁移记录移至 Migration Governance；Target 语义保持不变 | [#993](https://github.com/rushsinging/aemeath/issues/993) |
| 2026-07-17 | 明确 Registry Scope / capability Profile、只收缩与 `legacy-no-agent` 迁移目标；当前 #909 落地证据见 Migration Governance | [#909](https://github.com/rushsinging/aemeath/issues/909) |
