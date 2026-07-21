# Tool & Skill & Command · 端口与生命周期

> 层级：02-modules / tools（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#787（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Tool 双端口、ExecutionScope、协作取消、Skill/Command 协作边界与 MCP 生命周期。签名用于表达职责，不锁定具体 Rust API；当前实现证据与差距统一记录在 [Migration Governance](../../03-engineering/03-migration-governance.md)。

## 1. Tool 双端口

### 1.1 ToolCatalogPort

```rust
trait ToolCatalogPort: Send + Sync {
    fn snapshot(
        &self,
        scope: &RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError>;
}
```

职责：

- 根据 Registry Scope 与 Tool Profile 生成可见 Tool 投影；
- 返回 ToolDescriptor 与 input schema；
- 保证 ToolName 唯一、required resources 齐备、capabilities 被允许；
- 组合 built-in 与 MCP Tool，但隐藏来源实现。

禁止返回：

- `Arc<dyn Tool>` 或任何 Tool 实例；
- Registry、内部 handle 或函数指针；
- MCP client、transport、连接状态对象；
- RuntimeContext 或具体 Store。

### 1.2 ToolExecutionPort

```rust
trait ToolExecutionPort: Send + Sync {
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> ToolOutcome;
}
```

Execution 在调用瞬间重新验证：

1. Tool 当前已注册；
2. Tool Profile 只能通过基线构造或 `derive_restricted` 从父 Profile 派生，扩大 capability 集必须返回错误；
3. Invocation 的 Registry Scope 包含该 Tool；
4. Profile 允许该 Tool 的全部 capabilities；
5. required resources 可用；
6. input 符合当前 schema。

验证通过后调用实际函数并标准化 ToolOutcome。Tool 不存在统一返回 `ToolUnavailable`；schema 失败返回 `InvalidInput`；资源或 adapter 失败进入 `Failure(Internal | Unavailable)`。ToolExecutionPort 故意保持单一 ToolOutcome 错误通道，避免调用方在 `Result::Err` 与 `ToolOutcome::Failure` 之间产生两套失败语义。本阶段调用协议仅按 ToolName 定位，不携带 Tool ID、Tool Version 或 Catalog revision。

### 1.3 三阶段入口

| 阶段 | 唯一入口 | 必须验证 | 失败语义 |
|---|---|---|---|
| Scope 装配 | Composition Root 的 `RegistryScopeBuilder::build` | ToolName 唯一、schema 可解析、required resources 齐备 | 拒绝构建 Scope |
| Catalog 投影 | `ToolCatalogPort::snapshot` | Tool 属于 Scope、Profile capabilities 覆盖 required capabilities | 从 Snapshot 排除，并记录结构化诊断 |
| Execution | `ToolExecutionPort::execute` | Tool 仍存在、Profile 未扩权、resources 可用、input 符合当前 schema | `ToolOutcome::Failure` |

Scope/Profile 变化后必须重新构建或拉取 Snapshot；无论 Snapshot 是否新鲜，Execution 都执行最后检查。

## 2. Runtime 与 Tool 的职责分工

### 2.1 Runtime Tool Coordination

Runtime 拥有业务编排：

1. 从模型响应建立 Run 内 ToolCall 实体；
2. 调用 PolicyPort；
3. 触发 PreTool Hook；
4. 必要时请求用户审批；
5. 按 ToolDescriptor 的 concurrency declaration 编排多个调用；
6. 建立 cancellation 与 timeout；
7. 调用 ToolExecutionPort；
8. 若并发结果包含一个或多个 `ToolOutcome::Suspended`，先收集全部 outcome，再按原始 RunStep 的稳定 ToolCallId / 调用顺序逐个映射为 Runtime-owned interaction request；Run 任一时刻只能有一个 PendingInteraction；
9. 每个 reply / cancellation 收敛为对应 ToolCall 的最终 outcome 后才处理下一个 suspension；全部调用均终结后，Runtime 按原调用顺序触发 PostTool Hook、发布 Audit/Domain Event；
10. 处理重试与失败策略；
11. 将全部最终 ToolOutcome 转为 Run Step 结果，逐条做 L1 budget reduction，再以一次 `ContextPort::append_and_persist` 原子追加 assistant + 有序 tool results。

### 2.2 Tool BC

Tool BC 只拥有局部调用正确性：

- Scope/Profile/schema 再验证；
- required resources 解析；
- Tool 函数调用；
- 对 AskUser 等特殊函数产生 typed `ToolSuspension`，但不等待交互；
- 协作取消；
- ToolOutcome 标准化。

Policy、Hook、人工审批、timeout、跨 Tool 并发和重试不得下沉进 ToolExecutionPort，否则会吞并 Runtime 与其他 BC 的职责。

## 3. ExecutionScope 与资源端口

```rust
struct ExecutionScope {
    run_id: RunId,
    parent_run_id: Option<RunId>,
    workspace_id: WorkspaceId,
    workspace_root: WorkspaceRoot,
    invocation_source: InvocationSource,
    registry_scope: RegistryScopeName,
    profile: ToolProfileName,
    deadline: Option<SystemTime>,
}
```

ExecutionScope 是 Tool PL，**MUST** 只携稳定标识和值对象，**NEVER** 包含 RuntimeContext、Session、Registry、具体 Store、Project 实现类型、composition-only handle 或 channel。`workspace_id` / `workspace_root` 是调用开始时从 `WorkspaceRead` 取得的只读快照，**NEVER** 替代 live capability。

Project 已发布 `WorkspaceRead` / `WorkspaceControl`，Tool BC **MUST** 直接消费所需的 trait view，**NEVER** 定义第二层 Workspace façade。其精确签名以 [Project Workspace 端口](../project/02-ports-and-adapters.md) 为唯一真相；本文 **MUST** 只定义 Tool 的消费约束。其他资源仍按能力拥有者发布的窄端口装配：

```rust
trait FileAccess      { /* 工作区内文件能力 */ }
trait TaskAccess      { /* Task BC 发布能力 */ }
trait AgentDispatch   { /* 派生 Sub Run 的窄入口 */ }
```

AskUser 只需 `ToolCapability::UserInteraction` 授权，不需交互活资源：其 Tool adapter 将输入验证后返回 `ToolSuspension::UserInteraction`。Runtime 是 `InteractionPort`、等待状态、reply identity 与 cancellation 的唯一所有者；Tool Scope **NEVER** 注入第二套 `UserInteraction` trait。

Composition Root **MUST** 从当前 composition-internal workspace scope 的同一 Project wiring 取得窄 view，并按 Tool 实例而非整个 Registry Scope 广播能力；Tool adapter **NEVER** 接收 composition scope / wiring：

| Tool 实例 | 注入的 Project view |
|---|---|
| 文件 Tool（Read / Write / Edit / Glob / Grep） | `Arc<dyn WorkspaceRead>` |
| Bash | `Arc<dyn WorkspaceRead>` + `Arc<dyn WorkspaceControl>` |
| EnterWorktree / ExitWorktree | `Arc<dyn WorkspaceRead>` + `Arc<dyn WorkspaceControl>` |
| 其他 Tool | 仅 descriptor 明确声明且通过 capability 校验的 view |

只有 Bash、EnterWorktree、ExitWorktree **MAY** 获得 `WorkspaceControl`；只读文件 Tool **MUST** 只获得 `WorkspaceRead`。缺少 required resource 时，Tool 不进入该 Scope 的 Catalog 投影；Execution 时仍 **MUST** 重验 resource 与 capability。Tool adapter **NEVER** 接收 Project 的 production wiring handle。

## 4. Cancellation 与 timeout

```rust
trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
}
```

`CancellationSignal` 是 Tool PL，不绑定 Tokio。Runtime 将自己的 cancellation tree 适配为该接口。

职责边界：

- Runtime 决定 timeout 时长、超时后策略及父子 Run 传播；
- timeout 到期时 Runtime 发出 cancellation，并结束对 Tool future 的等待；
- Tool 协作停止子进程、网络请求或 MCP 调用；
- ToolDescriptor 声明是否支持协作取消；
- cancellation 不承载 timeout 或重试配置。

对无法协作取消且可能继续产生副作用的 Tool，Runtime 必须依据 Descriptor 限制并发并向用户明确风险。

## 5. Catalog Snapshot 与变化通知

ToolCatalogSnapshot 是当前 Scope/Profile 的只读视图。Catalog 来源变化时发布：

```rust
struct CatalogChanged {
    reason: CatalogChangeReason,
}
```

Runtime 收到事件后按需重新拉取 Snapshot；事件不直接携带 Registry 或 Tool 实例。事件传输使用通用 Event Port，不把 `tokio::sync::watch` 暴露为 Published Language。

本阶段不定义 Catalog revision。MCP 动态接线完成前，CatalogChanged 仅表达“重新拉取”的事实，不提供跨快照兼容承诺。

## 6. Skill 端口

```rust
trait SkillCatalogPort: Send + Sync {
    fn list(&self, query: SkillQuery) -> Vec<SkillDescriptor>;
}

#[async_trait]
trait SkillMaterializationPort: Send + Sync {
    /// 为一次 Context Window 构建返回已物化、已验证的快照。
    async fn materialize_available(
        &self,
        query: SkillMaterializationQuery,
    ) -> Result<SkillMaterializationSnapshot, SkillError>;
}

struct SkillMaterializationSnapshot {
    fragments: Vec<PromptFragment>,
    revision: SkillMaterializationRevision,
}
```

Skill Materializer 负责异步读取、解析与验证 Skill，输出带确定性内容 revision 的 PromptFragment 快照。Context Management 接收 Fragment 后决定注入位置、预算、去重、缓存分段和顺序。

文件系统 adapter 的入口契约：

- 标准 Skill 只识别 `<skill-dir>/SKILL.md`；同目录其他 Markdown 是资源，不参与 Catalog、Materialization 或 revision；
- package 只识别 `<package>/skills/<skill>/SKILL.md`，并应用 package namespace；
- 子目录同时存在直接 `SKILL.md` 与 `skills/` 时，直接入口优先，`skills/` 按该 Skill 的资源处理；
- 为兼容历史布局，只有 skills 根目录的直接 `*.md` 可继续作为扁平入口，禁止递归泛化；
- 扁平兼容面由 Tools filesystem adapter 负责；退役前必须另立迁移 Issue，审计并迁移真实使用方；
- 真实入口存在但读取、frontmatter 或 YAML 损坏时返回 typed `SkillError`，资源文件不进入 parser。

Skill 不是 Tool，不走 ToolExecutionPort；Context Management 不直接读取 Skill 文件或依赖其 adapter。Skill 的稳定 identity（`SkillDescriptor.name` / `PromptFragment.stable_key`）与用户可输入的 Slash 名称是不同概念：package namespace 只保证 identity 唯一，**NEVER** 自动成为 Slash Command。只有 `SkillDescriptor.slash_command` 显式存在且符合 Command PL 名称规则时，Composition 才将其投影为 PromptInjection `CommandDescriptor`；不暴露 Slash 的 Skill 仍可被 agent 发现与物化，且不得阻断 Command Catalog bootstrap。

## 7. Slash Command 端口

```rust
trait CommandCatalogPort: Send + Sync {
    fn list(&self) -> Vec<CommandDescriptor>;
    fn complete(&self, prefix: &str) -> Vec<CommandCompletion>;
}

trait CommandRouterPort: Send + Sync {
    fn resolve(&self, input: SlashInput) -> Result<CommandRoute, CommandParseError>;
}

enum CommandRoute {
    PromptInjection(PromptCommand),
    SnapshotQuery {
        target: SnapshotQueryTarget,
        command: SnapshotQueryCommand,
    },
    ApplicationControl {
        target: ApplicationControlTarget,
        command: ApplicationControlCommand,
    },
}
```

Router 只解析并选择机制，不统一执行三类命令，也不把各 BC 的结果折叠成通用基类。`ApplicationShell` 是封闭 target 中唯一的交付应用自身能力标识，仅用于 help、version、doctor、pending images、paste、exit 等不属于目标业务 BC 的命令；它 **NEVER** 包装 Runtime、Context、Memory、Config 等业务查询或状态变更：

- PromptInjection handler 将 PromptCommand 物化为 PromptFragment，再交给 Context Management；
- SnapshotQueryTarget 是封闭的目标 BC 标识；handler 依 target 调用对应 Query Port，并保留该 BC 的类型化 Published Snapshot；
- ApplicationControlTarget 是封闭的目标 BC 标识；handler 依 target 调用对应 Application Command Port，并保留该 BC 的类型化 Outcome。

CommandCatalog 的 `complete` 只根据 Descriptor/argument_schema 生成候选，不执行 route。Command 不直接读取 Runtime struct，不输出 terminal formatting。交付层通过 ACL 将各 BC 的 Published Snapshot/Outcome 转成 CLI、TUI 或 Server 展示。

## 8. MCP Adapter 与 ACL

MCP adapter 内部包含：

- MCP Connection 聚合；
- transport / JSON-RPC / authentication；
- server config 消费；
- tool discovery 与调用；
- MCP schema/annotations/result 到 Tool PL 的 ACL。

ACL 规则：

1. MCP Tool 名称必须规范化并进行重名检测；
2. input schema 映射为 Tool schema；
3. annotations 映射为 capabilities、concurrency 与 cancellation declaration；
4. 缺失或不可信 annotations 采用保守权限；
5. structured content、文本、图片与错误映射为 ToolOutcome；
6. transport error 不得直接泄漏密钥或协议私有信息。

## 9. MCP Connection 状态机

```text
Disabled ──enable──▶ Connecting
Connecting ──success──▶ Connected
Connecting ──failure──▶ Failed
Connected ──connection_lost──▶ Reconnecting
Connected ──disable──▶ Stopping ──stopped──▶ Disabled
Reconnecting ──success──▶ Connected
Reconnecting ──exhausted──▶ Failed
Failed ──retry──▶ Connecting    // Target 转换；retry 触发机制（backoff / 手动）在 §10 之后落地
Failed ──disable──▶ Disabled
```

只有 Connected 状态能发布 MCP Tool 投影。离开 Connected 时，连接聚合必须原子撤销对应投影并发布 CatalogChanged。

health check、自动重连、tool list changed、resource discovery 与 transport 清理都通过聚合命令推进状态，禁止外部直接写 state。

## 10. MCP 后续设计

以下契约在 MCP 动态接线阶段独立定案：

- 稳定 ToolId 与 server/tool rename；
- Tool/schema 版本与 Catalog revision；
- 动态上下线期间的 in-flight 兼容；
- MCP Resources 是否继续通过 Tool 暴露；
- health check 间隔、重连退避与资源上限的最终配置。

延后项不得用临时的名称黑名单或内部 handle 泄漏绕过。

## 11. Composition Root

Composition Root 负责：

- 注册 built-in Tool adapter；
- 根据 ConfigSnapshot 构造 Skill/Command catalog；
- 根据 RunSpec 构造 Registry Scope 和 Tool Profile；
- 从当前 `CompositionWorkspaceScope` 的 Project wiring 取得 `WorkspaceRead` / `WorkspaceControl` 窄 view，并按 Tool 实例注入；scope / wiring **NEVER** 进入 Tool 类型，且 **NEVER** 预建通用 Workspace wrapper；
- 为 Scope 注入其他资源端口；
- MCP 动态接线阶段构造连接聚合与 adapter；
- 向 Runtime/Context Management/交付层提供各独立端口。

架构守卫的目标规则在 #982 落地并故意违规验证，由 #763 汇总验收：

```text
Rule: tool-registry-construction-owned-by-composition
Scan: production Rust AST/path references to RegistryScopeBuilder::build,
      ToolRegistry::new and concrete Tool adapter constructors
Allow: agent/composition/** only
Deny: agent/features/** production code and apps/**
```

配套策略：

1. Registry、Scope builder 与具体 Tool adapter 构造器保持模块私有或 `pub(crate)`；composition 通过专用 adapter factory 访问，禁止公开 re-export；
2. capability policy 与用例代码不得依赖 composition 或具体 adapter；
3. 守卫的唯一白名单是 composition root 路径，新增白名单必须带 owner 与退出条件；
4. 装配测试断言 Sub Scope 的 Tool 集与 capability 集均不超过父 Scope/Profile；
5. #982 **MUST** 临时增加第二构造点证明守卫失败，再撤销违规。

## 12. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型：[01-domain-model.md](01-domain-model.md)
- Runtime Tool Coordination：[../runtime/02-module-boundaries.md](../runtime/02-module-boundaries.md)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Project Workspace 端口：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 依赖规则：[../../01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：双 Tool 端口、ExecutionScope、取消、Skill/Command 协作与 MCP 生命周期 | #787 |
| 2026-07-20 | 明确 Skill stable identity / identity aliases 与显式 Slash name / slash aliases 分离；package namespace Skill 默认不投影为 Slash Command，外部 Skill 元数据不得阻断 Command Catalog bootstrap | #1302 |
| 2026-07-14 | Tool 资源改为直接消费 Project-owned WorkspaceRead / WorkspaceControl，移除宽包装并将 Control 注入限于三个 Tool | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #993 过渡目录迁移记录移至 Migration Governance；本文端口与生命周期保持 Target | [#993](https://github.com/rushsinging/aemeath/issues/993) |
| 2026-07-17 | #909 补充 Scope/Profile 与只收缩目标的承接关系；当前实现边界见 Migration Governance | [#909](https://github.com/rushsinging/aemeath/issues/909) |
