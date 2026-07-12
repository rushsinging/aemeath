# Agent Runtime · 端口与适配器

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Agent Runtime 的入站端口、12 个出站端口、RuntimeContext 装配与 Composition Root。**只描述目标态**；现状端口缺口（12 个只有 4 个有 trait）记入 `03-engineering/migration-governance`。

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

## 2. 出站端口清单（12 个，签名草案）

```rust
trait ContextPort {                                  // Context Management BC
    fn build_window(&self, run: &Run) -> ContextWindow;   // 历史+compact+注入+prompt
    fn needs_compaction(&self, run: &Run) -> bool;
    fn compact(&self, run: &mut Run, cancel: &RunCancellationScope);
}
trait ProviderPort {                                 // Provider BC（内部 ACL）
    fn invoke(&self, window: ContextWindow, effort: ReasoningLevel,
              cancel: &RunCancellationScope)
        -> Stream<InvocationDelta>;                       // 流式
}
trait ToolPort {                                     // Tool & Skill & Command BC
    fn execute(&self, calls: Vec<ToolCall>, cancel: &RunCancellationScope)
        -> Vec<ToolResult>;
    fn schemas(&self) -> Vec<ToolSchema>;                 // 受限 registry 的可用工具
}
trait PolicyPort {                                   // Policy BC
    fn check(&self, call: &ToolCall) -> PolicyDecision;   // Allowed/Denied/NeedAsk
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
trait WorkspacePort {                                // Project BC（Sub: 独立快照）
    fn current_frame(&self) -> WorkspaceFrame;
    fn seed_isolated(&self) -> WorkspaceFrame;            // 快照父 frame
}
trait HookPort {                                     // Hook BC（Sub: BoundaryOnly）
    fn run(&self, point: HookPoint, ctx: HookContext,
           cancel: &RunCancellationScope) -> HookOutcome;
}
trait ReasoningPort {                                // Workflow BC（Sub: EffortOnly）
    fn effort(&self, run: &Run) -> ReasoningLevel;
}
trait AuditSink {                                    // Audit BC（Pub/Sub，新增）
    fn emit(&self, event: AuditEvent);                    // 执行/成本事件
}
trait EventSink {                                    // 事件出口（Main→TUI / Sub→父）
    fn emit(&self, events: Vec<DomainEvent>);
}
// ConfigSnapshot：只读快照（Config BC 的 PL），Main/Sub 共享
```

## 3. RuntimeContext 装配

`RuntimeContext` 持有以上 12 个端口的**活实例**（Config/Event 除外为快照/sink）。装配规则由 `RunSpec` 驱动：

```rust
fn assemble(spec: &RunSpec, parent: Option<&RuntimeContext>, root: &CompositionRoot)
    -> RuntimeContext
{
    RuntimeContext {
        context:   root.context_for(spec.context),        // Isolated → 独立 manager
        provider:  root.provider_for(&spec.model, spec),  // Sub → 独立 client 副本
        tools:     root.tools_for(&spec.tools),           // 受限 registry（只收缩）
        policy:    match spec.policy {
                       Direct => root.policy(),
                       DelegatedApproval => Delegated::new(root.policy(), parent), // 设计态
                   },
        memory:    match spec.memory { Enabled => root.memory(), Disabled => NoOpMemory },
        task:      match spec.task { Shared => root.task(), Isolated => TaskStore::new().into() },
        workspace: match spec.workspace {
                       Inherit => parent_or_root_frame(),
                       Snapshot => root.workspace().seed_isolated(),
                   },
        hooks:     match spec.hooks { Full => root.hooks(), BoundaryOnly => Boundary::new(root.hooks()), Disabled => NoOpHooks },
        reasoning: match spec.reasoning { GraphDriven => root.reasoning(), EffortOnly => Effort::new(inherit(parent)), Inherit => parent_effort() },
        audit:     root.audit(),
        config:    root.config_snapshot(),                // 共享
        input:     match spec.name.as_ref() { "main" => root.tui_input(), _ => FixedQueue::new(spec.initial_prompt) }, // 仅内容输入
        events:    match spec.name.as_ref() { "main" => root.tui_sink(), _ => ParentRunSink::new(parent) },
        cancel:    match parent { Some(ctx) => ctx.cancel.child_scope(), None => RunCancellationScope::new() },
    }
}
```

**安全铁律落地**（见 [01-domain-model.md](01-domain-model.md) §7）：`tools_for` 只能收缩不能扩张；`policy` 不放宽；`workspace` 强制 `seed_isolated`。

## 4. Composition Root

- **唯一生产装配入口**：`agent/composition`。持有各 Port 的具体实现（provider driver / tool registry / storage / git / hook …），提供 `assemble()` 所需的 `root.*()` 工厂。
- Main Run：由 Composition Root 直接 `assemble(main_spec, None, root)`
- Sub Run：由 tool_coordination 派生时 `assemble(sub_spec, Some(parent_ctx), root)`
- **MUST NOT** 任何模块私自 `new` Port 实现绕过 Composition Root

## 5. 关键 ACL

1. **Provider 内部**：各家 LLM API → 统一 `InvocationDelta` + 领域 `Message`
2. **event_projection**：领域 `DomainEvent` → SDK `ChatEvent`（Main/Sub 路由 + agent_id）
3. **Session 快照组装**：落盘时经 TaskPort/WorkspacePort 收快照内嵌

## 6. 现状端口缺口（迁移提示）

| 目标端口 | 现状 | 迁移动作（S5）|
|---|---|---|
| ContextPort / ToolPort / PolicyPort / MemoryPort / WorkspacePort / ReasoningPort | ❌ 无 trait，具体类型直调 | 抽 trait，实现移到适配器 |
| AuditSink | ❌ 完全无 | 新建（Pub/Sub） |
| ProviderPort | ⚠️ 仅 `ProviderInfoPort`（只读元数据）| 补 invoke 方法 |
| HookPort | ⚠️ 仅 `HookNotificationPort` | 补 per-tool run |
| TaskPort / ConfigSnapshot / EventSink | ✅ `TaskStorePort`/`ConfigReader`/`ChatEventSink` | 沿用 |
| EventSink agent_id | ⚠️ 事件仅 chat_id/turn_id | 补 agent_id（#612）|
| cancel_run / RunCancellationScope | ⚠️ SDK `CancelHandle` 捕获 Session 级 `Arc<Mutex<CancellationToken>>`，每回合替换；另有 `ChatInputEvent::Cancel` | 改为绑定 `run_id` 的同步幂等入口 + per-Run scope；旧事件退役（#700）|
| Context/Provider/Tool/Hook cancellation | ⚠️ Provider/Tool 部分监听 token；compact 创建独立 token；Hook 仅 timeout | 全部接收同一 Run scope 或派生 scope（#700/S5）|

## 7. 相关文档

- 领域模型（RunSpec/RuntimeContext）：[01-domain-model.md](01-domain-model.md)
- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- 上下文地图（BC 集成）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 系统架构（Composition Root）：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：入站端口、12 出站端口签名、RuntimeContext 按 RunSpec 装配、Composition Root、ACL、现状缺口 | #761 |
| 2026-07-11 | RuntimeContext/assemble 补入站端口 InputBuffer（Main=TUI 通道+buffer，Sub=固定队列）| #761 |
| 2026-07-12 | 定义同步幂等 `cancel_run(run_id)`、per-Run cancellation scope 及 Provider/Tool/Compact/Hook 传播边界 | #700 |
