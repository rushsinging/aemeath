# Agent Runtime · 端口与适配器

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Agent Runtime 的入站端口、出站端口、RuntimeContext 装配与 Composition Root。**只描述目标态**；实现缺口记入 `03-engineering/migration-governance`。

## 1. 入站端口（OHS + Published Language）

`AgentClient` trait（`packages/sdk`）= 核心域对外的入站端口 + 发布语言，供 CLI/TUI/Server 消费。所有权属 Agent Runtime，独立成 crate 仅为依赖倒置。契约细节见 [../../01-system/03-context-map.md](../../01-system/03-context-map.md)。

### 同步打断入口

```rust
trait AgentClient {
    // 其他命令省略
    fn cancel_run(&self, run_id: RunId) -> CancelRunOutcome;
}

enum CancelRunOutcome {
    Accepted,       // 调用返回前已进入 Cancelling 且 cancellation scope 已触发
    AlreadyCancelling,
    AlreadyTerminal,
    NotFound,
}
```

- `cancel_run` 是同步、幂等、out-of-band 的控制命令，NEVER 经 `InputBuffer` 排队。
- TUI 只持 `Arc<dyn AgentClient>` 或 SDK 提供的、绑定 `run_id` 的薄 `CancelHandle`；NEVER 持有 Runtime 实例、Run 聚合或 `CancellationToken`。
- `CancelHandle::cancel()` 只是 `cancel_run(run_id)` 的语法便利，生命周期绑定单个 Run，NEVER 捕获 Session 级可替换 token 槽。
- 返回 `Accepted` 只确认取消请求已即时生效；取消完成由 `RunCancelled` 事件异步确认。
- 对未带 `run_id` 的旧 `ChatInputEvent::Cancel`，迁移期只能映射到同一个 `cancel_run(active_run_id)`，完成 SDK 迁移后退役，NEVER 保留第二套语义。

## 2. 出站端口清单（签名草案）

```rust
trait ContextPort: Send + Sync {                                  // Context Management BC
    /// 构建本轮 Context Window（L2 snip + L3 microcompact + L4 collapse + memory 注入 + prompt 组装）
    /// L2/L3/L4 均为读模型变换，不修改 ChatChain
    fn build_window(&self, req: &ContextRequest) -> ContextWindow;
    /// 判断是否需要 auto-compact（幂等）
    fn needs_compaction(&self, req: &ContextRequest) -> CompactionDecision;
    /// L5 auto-compact：LLM 摘要替换历史（唯一修改 ChatChain 的压缩策略）
    fn compact(
        &self,
        chain: &mut ChatChain,
        req: &ContextRequest,
        cancellation: &dyn CancellationSignal,
    ) -> CompactResult;
}
// 详见 context-management/02-compact.md
trait ProviderPort: Send + Sync {                    // Provider BC（内部 ACL）
    fn capabilities(&self, model: &ModelId)
        -> Result<ModelCapability, ProviderError>;
    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &dyn CancellationSignal,
    ) -> Result<InvocationStream, ProviderError>;     // 单次 attempt 的有序流
}
trait ToolCatalogPort {                              // Tool BC：只读目录投影
    fn snapshot(&self, scope: RegistryScopeName, profile: ToolProfileName)
        -> ToolCatalogSnapshot;
}
trait ToolExecutionPort {                            // Tool BC：单次函数调用
    fn execute(&self, invocation: ToolInvocation, cancellation: &dyn CancellationSignal)
        -> ToolOutcome;
}
// SkillCatalogPort / SkillMaterializationPort 面向 Context Management；
// CommandCatalogPort / CommandRouterPort 面向 CLI/TUI/Server，不进入 RuntimeContext 的 Tool 执行路径。
trait PolicyPort {                                   // Policy BC（v0.1.0: AllowAllPolicy）
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision;
}
trait MemoryPort {                                   // Memory BC（Sub: NoOp）
    fn retrieve(&self, query: &MemoryQuery) -> Vec<MemoryEntry>;
    fn write(&self, entry: MemoryEntry);
}
trait TaskPort {                                     // Task BC（Sub: 独立实例）
    fn snapshot(&self) -> TaskSnapshot;
    fn restore(&self, snap: TaskSnapshot);
    fn list_current(&self) -> Vec<Task>;
}
trait HookPort {                                     // Hook BC：一个类型化端口
    fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcome;
}
trait ReasoningPort {                                // Workflow BC（Sub: EffortOnly）
    fn effort(&self, run: &Run) -> ReasoningLevel;
}
trait UsageSink {                                    // Audit BC（MVP Pub/Sub）
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}
trait EventSink {                                    // 事件出口（Main→TUI / Sub→父）
    fn emit(&self, events: Vec<DomainEvent>);
}
// ConfigSnapshot：只读快照（Config BC 的 PL），Main/Sub 共享
```

## 3. RuntimeContext 与 CompositionRunScope 装配

`RuntimeContext` **MUST** 只持有 Runtime 消费的活端口（Config 为快照、Event/Audit 为 sink），**NEVER** 持有 Workspace trait、Project wiring 或 composition scope。`WorkspaceMode` 由 Composition 消费，在同一次装配中生成 runtime context 与 composition-internal Run scope：

```rust
// agent/composition 内部类型；NEVER 出现在任一 feature 的公开 API。
struct CompositionRunScope {
    workspace: WorkspaceWiring,
}

// 同为 composition 内部返回值；启动 Run 后 scope 仍由 Composition 保留。
struct AssembledRun {
    runtime: RuntimeContext,
    scope: CompositionRunScope,
}

fn assemble(
    spec: &RunSpec,
    parent_runtime: Option<&RuntimeContext>,
    parent_scope: Option<&CompositionRunScope>,
    root: &CompositionRoot,
) -> AssembledRun {
    let scope = root.run_scope_for(spec.workspace, parent_scope);

    // 两个 backing implementation 从同一 Project wiring 取得窄 view。
    let context = root.context_for(
        spec.context,
        scope.workspace.read(),
        scope.workspace.persist(),
    );
    let (tool_catalog, tool_execution) = root.tools_for(
        &spec.tools,
        scope.workspace.read(),
        scope.workspace.control(),
    );

    let runtime = RuntimeContext {
        context,
        provider:  root.provider_for(&spec.model, spec),
        tool_catalog,
        tool_execution,
        policy:    root.allow_all_policy(),
        memory:    match spec.memory { Enabled => root.memory(), Disabled => NoOpMemory },
        task:      match spec.task { Shared => root.task(), Isolated => TaskStore::new().into() },
        hooks:     root.hooks(),
        reasoning: root.reasoning_for(spec.reasoning, parent_runtime),
        usage:     root.usage_sink(),
        config:    root.config_snapshot(),
        input:     root.input_for(spec),
        events:    root.event_sink_for(spec, parent_runtime),
        cancel:    root.cancel_scope_for(parent_runtime),
    };

    AssembledRun { runtime, scope }
}
```

`run_scope_for` **MUST** 只接受两种生产组合：Main 的 `(parent_scope = None, WorkspaceMode::Inherit)` 调用 Project production factory；Sub 的 `(Some(parent_scope), WorkspaceMode::Snapshot)` 调用 `parent_scope.workspace.derive_isolated()`。其他组合 **MUST** 作为无效 RunSpec 拒绝。由此，Context Management 与 Tool backing implementation 获得的是同一个 Main / Sub workspace 实例的不同窄 view。

注入 dispatch Tool 的 composition-provided `AgentDispatch` / Sub-run factory **MUST** 捕获父 `CompositionRunScope`，或以 RunId 在 composition-private registry 中索引它，再调用同一 `assemble`。`CompositionRunScope` / `AssembledRun` **NEVER** 进入 RuntimeContext、ToolInvocation、ContextRequest、ContextPort 或 ToolExecutionPort。

**安全铁律落地**（见 [01-domain-model.md](01-domain-model.md) §7）：Registry Scope 只能移除 Tool/Resource，Tool Profile 的 capability 集只能收缩；`policy` 不放宽；Sub workspace **MUST** 通过 parent scope 的 `derive_isolated()` 派生。

## 4. Composition Root

- **唯一生产装配入口**：`agent/composition`。持有各 Port 的具体实现或模块提供的 composition-only opaque wiring（provider driver / tool registry / storage / workspace / hook …），提供 `assemble()` 所需的 `root.*()` 工厂。
- Project workspace 的生产装配 **MUST** 经 Project-owned factory 取得 `WorkspaceWiring`，并 **MUST** 只保存在 `CompositionRunScope`；Composition **NEVER** 向 Runtime 或业务模块分发 handle / scope。
- Main Run：Composition Root 直接建立 Main scope，再装配 RuntimeContext 与 Context / Tool backing implementation。
- Sub Run：dispatch Tool 经 composition-provided AgentDispatch 从 parent scope 执行 `derive_isolated()`，再装配相同结构；Runtime tool coordination 只编排 `ToolExecutionPort` 调用。
- 任何模块 **NEVER** 私自 `new` Port 实现绕过 Composition Root。

## 5. 关键 ACL

1. **Provider 内部**：各家 LLM API → 统一 `InvocationDelta` + 领域 `Message`
2. **event_projection**：领域 `DomainEvent` → SDK `ChatEvent`（Main/Sub 路由 + agent_id）
3. **Session 快照组装**：Context Management backing implementation 直接经注入的 `TaskPort` / Project-owned `WorkspacePersist` 收集与恢复；Runtime **NEVER** 中转 Workspace 能力
4. **Workspace scope 隔离**：Composition 从同一 `CompositionRunScope` 分发 Project 窄 view；scope / wiring **NEVER** 穿过 Runtime、Tool 或 Context 边界

## 6. 迁移边界

本文 **MUST** 只定义 Target 契约。Runtime 出站端口、取消链路与 composition-internal Run scope 的 Current → Target 差距、责任和退出条件 **MUST** 只在 [迁移治理](../../03-engineering/migration-governance.md) 维护，**NEVER** 在本文复制进度表。

## 7. 相关文档

- 领域模型（RunSpec/RuntimeContext）：[01-domain-model.md](01-domain-model.md)
- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- Context Management 战术设计（ContextPort/MemoryPort/PromptPort 详解）：[../context-management/02-compact.md](../context-management/02-compact.md)
- 上下文地图（BC 集成）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 系统架构（Composition Root）：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)
- Provider 端口、流与 Invocation Scope：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Project Workspace 端口与 wiring：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：入站端口、出站端口签名、RuntimeContext 按 RunSpec 装配、Composition Root、ACL、实现缺口 | #761 |
| 2026-07-11 | RuntimeContext/assemble 补入站端口 InputBuffer（Main=TUI 通道+buffer，Sub=固定队列）| #761 |
| 2026-07-12 | 定义同步幂等 `cancel_run(run_id)`、per-Run cancellation scope 及 Provider/Tool/Compact/Hook 传播边界 | #700 |
| 2026-07-12 | ToolPort 拆为 Catalog/Execution 双端口，补 Skill/Command 独立端口边界与 Scope/Profile 装配 | #787 |
| 2026-07-12 | ProviderPort 补能力查询、取消、结构化错误与单 attempt InvocationStream 契约 | #788 |
| 2026-07-12 | ContextPort 签名更新为 4 方法（build_window/microcompact/needs_compaction/compact），详见 context-management/02-compact.md | #786 |
| 2026-07-12 | Policy 装配收缩为 AllowAll；Hook 收敛单 dispatch；Audit 出站收缩为非阻塞 UsageSink | #790 |
| 2026-07-14 | 移除 Runtime Workspace 端口；由 CompositionRunScope 保留 Project wiring，Sub 在 AgentDispatch 内派生，并从同一实例装配 Context / Tool 窄能力 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
