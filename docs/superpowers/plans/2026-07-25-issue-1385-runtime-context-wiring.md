# #1385 RuntimeContext 生产接线实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 `RunSpec → RuntimeContext → Run` 校正资源作用域，使 RuntimeContext 成为 Main/Sub 生产路径唯一的 per-Run 活契约容器，删除 `RuntimeResources`，并为 #1397 提供稳定生产入口。

**Architecture:** RuntimeContext 只持单次 Run 使用的活契约与 cancellation scope；MainSessionWiring、Workspace composition scope、session identity、idle query、model switch、prompt bootstrap 和其他 session shell 状态继续由 Main Session shell 持有。Main 由 Composition 提供父能力并按 `RunSpec::main()` 装配；Sub 必须从父 `RunSpec` 与父 RuntimeContext 收缩派生，工具、内存、交互、事件、workspace 等能力只允许收缩或平移。本 Issue 只完成资源接线，不提取 `RunExecutionState`，不删除 fat `MainRunPort` / `SubAgentRun`。

**Tech Stack:** Rust 2021、Tokio、async-trait、runtime/context/tools/memory/task/workflow/composition crates、Cargo workspace architecture guards

**Issue:** [#1385](https://github.com/rushsinging/aemeath/issues/1385)

---

## 1. Issue 门禁清单

实施期间必须逐项维护；创建 PR 前每项必须完成，或在 PR 中记录可验证的不适用理由。

- [ ] `RuntimeContext` 是 Main/Sub 生产路径唯一 per-Run 活资源容器。
- [ ] `RuntimeResources` 删除，且生产代码零引用。
- [ ] `ChatLoopContext` 不再复制 service 字段，收敛为 Main Session shell/launch input；不得提前承接 `RunExecutionState`。
- [ ] 4 个 `Arc<Fn>` 查询收敛为 `SessionQueryPort`，且该 port 不进入 RuntimeContext。
- [ ] Workspace wiring、`MainSessionWiring`、Composition scope 不进入 RuntimeContext。
- [ ] Main/Sub RuntimeContext 均由 `RunSpec` 驱动装配；Sub 能力只能收缩或平移。
- [ ] Main/Sub 每层装配契约、runtime/composition 测试、clippy 与架构守卫通过。
- [ ] 不删除 `MainRunPort`、`SubAgentRun`、`RunLoopPort`；该退役工作留给 #1397/#1399。
- [ ] 不自行关闭 Issue；合入后等待用户确认。

## 2. 作用域分类与文件结构

### 2.1 RuntimeContext 允许持有

以当前生产可用类型为准，建立 `RuntimeContextParts` 后一次构造：

- `Arc<ContextCoordinator>` 或其唯一 ContextPort backing
- `Arc<ProviderBinding>`（其中包含 `Arc<dyn ProviderPort>` 和调用冻结属性）
- `Arc<dyn ToolCatalogPort>` / `Arc<dyn ToolExecutionPort>`
- `Arc<dyn ToolExecutionContextBindingPort>`
- `Arc<dyn policy::PolicyPort>`
- `Arc<InteractionBridge>`
- `Arc<dyn memory::MemoryPort>` / reflection 活契约
- `Arc<dyn task::TaskAccess>`
- `Arc<dyn hook::HookPort>`
- `Arc<dyn ReasoningPort>`
- 当前生产可用的 usage sink 或明确的 no-op adapter
- per-Run input/event seam
- `RunConfigSnapshot`
- `RunCancellationScope`

不得为了形式对齐而保留没有生产 adapter 的旧空壳端口；先使用当前生产 Published Language，后续 ownership issue 再替换类型。

### 2.2 Main Session shell 保留

- `MainSessionWiring`、session identity、resume/verbose
- `project::WorkspaceViews` 与 Composition workspace scope
- provider factory、当前可切换 binding、model switch
- `SessionQueryPort`
- prompt bootstrap：system blocks、initial git context、user guidance、skills view
- session reminders、read files、agent concurrency、agent runner、tool-result materializer
- SDK/TUI launch projection所需状态

### 2.3 计划涉及文件

| 文件 | 职责 | 操作 |
|---|---|---|
| `agent/features/runtime/src/ports/session_query.rs` | Main idle query typed port | 新建 |
| `agent/features/runtime/src/ports/session_query_tests.rs` | query port adapter contract | 新建 |
| `agent/features/runtime/src/ports.rs` | 注册 SessionQueryPort；退役 WorkspacePort export | 修改 |
| `agent/features/runtime/src/ports/workspace_port.rs` | 无生产实现的过期端口 | 删除 |
| `agent/features/runtime/src/application/runtime_context.rs` | per-Run 活契约容器与父子派生入口 | 重写 |
| `agent/features/runtime/src/application/runtime_context_tests.rs` | scope、identity、cancel、不可扩权测试 | 新建 |
| `agent/features/runtime/src/application/client/session_query.rs` | AgentClient session query adapter | 新建 |
| `agent/features/runtime/src/application/client.rs` | 注册 adapter module | 修改 |
| `agent/features/runtime/src/application/client/accessors.rs` | Main Session shell / RuntimeHandle | 修改 |
| `agent/features/runtime/src/application/client/from_args.rs` | bootstrap shell 依赖，不再构造 RuntimeResources | 修改 |
| `agent/features/runtime/src/application/client/trait_chat.rs` | 注入 SessionQueryPort 与 launch input | 修改 |
| `agent/features/runtime/src/application/main_loop/looping/loop_context.rs` | 收敛为 shell + launch input | 修改 |
| `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs` | 每个 Main Run 按 RunSpec 装配 RuntimeContext | 修改 |
| `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs` | 消费 `&RuntimeContext`，保留执行状态 | 修改 |
| `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs` | Main idle/run 装配回归 | 修改 |
| `agent/features/runtime/src/application/subagent/runner/setup.rs` | 从父能力派生 Sub RuntimeContext | 修改 |
| `agent/features/runtime/src/application/subagent/runner/loop_run.rs` | 消费 Sub RuntimeContext，RunSpec 单一来源 | 修改 |
| `agent/features/runtime/src/application/subagent/runner/tests.rs` | Sub 收缩装配与运行回归 | 修改 |
| `agent/features/runtime/src/application/resources.rs` | 旧资源复制容器 | 删除 |
| `agent/features/runtime/src/application.rs` | 删除 resources module | 修改 |
| `agent/features/runtime/src/ports/legacy.rs` | `ChatRuntimeContext` 退化为启动参数或删除 | 修改 |
| `agent/features/runtime/src/application/service.rs` | 适配启动参数变化 | 修改 |
| `agent/composition/src/runtime.rs` | 注入 Main Session shell 父能力 | 修改 |
| `agent/composition/tests/runtime_context_wiring.rs` | Composition→Runtime 装配契约 | 新建 |
| `agent/features/runtime/tests/main_session_wiring_integration.rs` | Main 跨层场景回归 | 修改 |

---

## Task 1：建立 `SessionQueryPort`，替换四个查询闭包

**Issue 门禁：** 4 个 `Arc<Fn>` 收敛为一个 typed port，且不进入 RuntimeContext。

**Files:**
- Create: `agent/features/runtime/src/ports/session_query.rs`
- Create: `agent/features/runtime/src/ports/session_query_tests.rs`
- Modify: `agent/features/runtime/src/ports.rs`
- Create: `agent/features/runtime/src/application/client/session_query.rs`
- Modify: `agent/features/runtime/src/application/client.rs`
- Modify: `agent/features/runtime/src/application/client/trait_chat.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_context.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

- [ ] **Step 1：写失败契约测试**

在 `session_query_tests.rs` 定义 recording fake，实现四个 async 方法；分别调用 `list_models`、`list_sessions`、`list_reminders`、`list_reflection_history(7)`，断言返回值和调用参数。测试必须证明一个对象承载四类查询，不能只做源码字符串检查。

生产 trait 固定为：

```rust
#[async_trait::async_trait]
pub trait SessionQueryPort: Send + Sync {
    async fn list_models(&self) -> Result<Vec<sdk::ModelSummary>, sdk::SdkError>;
    async fn list_sessions(&self) -> Result<Vec<sdk::SessionSummary>, sdk::SdkError>;
    async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, sdk::SdkError>;
    async fn list_reflection_history(
        &self,
        limit: usize,
    ) -> Result<Vec<sdk::ReflectionHistoryView>, sdk::SdkError>;
}
```

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib session_query -- --nocapture`

Expected: FAIL，`session_query` 模块或 `SessionQueryPort` 尚不存在。

- [ ] **Step 3：实现 port 与 AgentClient adapter**

在 `session_query.rs` 文件末尾注册外置测试：

```rust
#[cfg(test)]
#[path = "session_query_tests.rs"]
mod tests;
```

`application/client/session_query.rs` 中的 adapter 只持 `Arc<RuntimeHandle>`，四个方法分别委托现有 `trait_model::*_impl`、`trait_session::*_impl`、`trait_memory::*_impl`、`trait_reflection::*_impl`。它属于 Main Session shell，不得被 `RuntimeContext` 引用。

- [ ] **Step 4：替换 ChatLoopContext 四个闭包**

将四个字段替换成：

```rust
pub session_queries: Arc<dyn crate::ports::SessionQueryPort>,
```

loop idle 分支调用对应 trait 方法。将测试里的四组 closure fixture 合并为一个 fake port。

- [ ] **Step 5：运行 GREEN**

Run: `cargo test -p runtime --lib session_query -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --lib loop_runner_tests -- --nocapture`

Expected: PASS，idle `/models`、`/sessions`、`/reminders`、reflection history 行为不变。

- [ ] **Step 6：提交**

```bash
git add agent/features/runtime/src/ports.rs \
  agent/features/runtime/src/ports/session_query.rs \
  agent/features/runtime/src/ports/session_query_tests.rs \
  agent/features/runtime/src/application/client \
  agent/features/runtime/src/application/main_loop/looping
git commit -m "refactor(runtime): #1385 收敛 session query port"
```

---

## Task 2：冻结 RuntimeContext 的生产契约与禁止字段

**Issue 门禁：** RuntimeContext 只持 per-Run 活契约；Workspace/MainSessionWiring/Composition scope 不进入。

**Files:**
- Modify: `agent/features/runtime/src/application/runtime_context.rs`
- Create: `agent/features/runtime/src/application/runtime_context_tests.rs`
- Modify: `agent/features/runtime/src/application.rs`
- Delete: `agent/features/runtime/src/ports/workspace_port.rs`
- Modify: `agent/features/runtime/src/ports.rs`

- [ ] **Step 1：写 RuntimeContext L1/L2 失败测试**

测试至少覆盖：

1. `main_runtime_context_preserves_injected_port_identity`：构造 main context 后，Context、Provider、Tool Catalog/Execution、Policy、Interaction、Memory、Task、Hook、Reasoning 的 `Arc::ptr_eq` 均成立。
2. `runtime_context_cancel_scope_is_per_run`：两个 Main context token 不共享；父 scope 取消能传播到 child。
3. `runtime_context_api_exposes_no_workspace_or_session_wiring`：在 Task 9 使用架构守卫和精确 `rg` 规则证明 `runtime_context.rs` 不引用 Workspace/MainSessionWiring/SessionQueryPort；本 L2 测试只验证公开 accessor 集合和真实资源 identity，不引入 workspace 未使用的 compile-fail 依赖。

生产构造输入收敛为单参数 parts：

```rust
pub struct RuntimeContextParts {
    pub context: Arc<dyn ContextPort>,
    pub provider: Arc<ProviderBinding>,
    pub tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    pub tool_execution: Arc<dyn tools::ToolExecutionPort>,
    pub tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    pub policy: Arc<dyn policy::PolicyPort>,
    pub interaction: Arc<InteractionBridge>,
    pub memory: Arc<dyn memory::MemoryPort>,
    pub reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    pub task: Arc<dyn task::TaskAccess>,
    pub hooks: Arc<dyn hook::HookPort>,
    pub reasoning: Arc<dyn workflow::api::ReasoningPort>,
    pub usage: Arc<dyn UsageSink>,
    pub input: Arc<dyn InputBuffer>,
    pub events: Arc<dyn EventSink>,
    pub config: RunConfigSnapshot,
    pub cancel: RunCancellationScope,
}
```

`task` 明确从旧空壳 `TaskPort` 校正为生产已使用的低权限 `TaskAccess`（#889）；`reflection_history` 使用现有 `AtomicDatasetReflectionHistoryStore`，`usage` 使用现有生产 sink，若 Composition 当前尚未注入 usage，则本 Task 新增显式 no-op `UsageSink` adapter 并补契约测试，禁止保留未接线 trait 空壳。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib runtime_context_tests -- --nocapture`

Expected: FAIL，现有 RuntimeContext 仍含 WorkspacePort，且缺少 production parts/identity API。

- [ ] **Step 3：重写 RuntimeContext**

要求：

- 所有字段私有。
- 对 RunLoop adapter 需要共享 ownership 的契约提供 `Arc` clone accessor；只需借用时返回 `&dyn`。
- `RunCancellationScope` 包装 `CancellationToken`，Main 使用 `new()`，Sub 仅可通过 `child_scope()` 派生。
- 删除 `WorkspacePort` 文件与 re-export。
- 文件末尾按规范引用外置测试：

```rust
#[cfg(test)]
#[path = "runtime_context_tests.rs"]
mod tests;
```

- [ ] **Step 4：运行 GREEN 与生产编译**

Run: `cargo test -p runtime --lib runtime_context_tests -- --nocapture`

Expected: PASS。

Run: `cargo build -p runtime`

Expected: exit 0；生产 target 不因测试引用掩盖 dead code。

- [ ] **Step 5：提交**

```bash
git add agent/features/runtime/src/application/runtime_context.rs \
  agent/features/runtime/src/application/runtime_context_tests.rs \
  agent/features/runtime/src/application.rs \
  agent/features/runtime/src/ports.rs \
  agent/features/runtime/src/ports/workspace_port.rs
git commit -m "refactor(runtime): #1385 冻结 per-run runtime context 契约"
```

---

## Task 3：补全 RunSpec 子能力偏序和派生不变量

**Issue 门禁：** Sub 能力只能收缩或平移。

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/spec.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/tests.rs`

- [ ] **Step 1：写表驱动失败测试**

扩展现有 `derived_sub_spec_can_only_restrict_parent_capabilities`，覆盖以下偏序：

| 能力 | 父到子允许方向 | 禁止方向 |
|---|---|---|
| input | SessionQueue → Fixed | Fixed → SessionQueue |
| interaction | Interactive → NonInteractive | NonInteractive → Interactive |
| events | Client → ParentRun | ParentRun → Client |
| context/workspace | Shared → Isolated | Isolated → Shared |
| memory | Enabled → Disabled | Disabled → Enabled |
| tools | Full → Restricted | Restricted → Full |

取消不是 RunSpec 字段；父 root → child 的 cancellation 不变量由 Task 2/Task 6 的 RuntimeContext 派生测试覆盖。

另加 `nested_sub_derivation_never_restores_parent_capability`，从已收缩 Sub 再派生时不得恢复能力。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib derived_sub_spec -- --nocapture`

Expected: input/interaction/events/context/workspace 新增限制 API 尚不存在或断言失败，即为 RED；memory/tools 已有护栏的断言可以直接通过。

- [ ] **Step 3：实现偏序校验**

将能力比较定义一次，禁止分别在 builder/setup 重复编码。`derive_sub` 生成默认 Sub spec 后调用统一的 `ensure_not_escalated_from(parent)`。timeout 规则：Main 的 0 表示无限；有限父 timeout 下声明值满足 `child.timeout <= parent.timeout`。运行时剩余 deadline 的传播不属于纯 RunSpec 校验，由既有 cancellation/launcher 负责。

- [ ] **Step 4：运行 GREEN**

Run: `cargo test -p runtime --lib agent_run -- --nocapture`

Expected: PASS。

- [ ] **Step 5：提交**

```bash
git add agent/features/runtime/src/domain/agent_run/spec.rs \
  agent/features/runtime/src/domain/agent_run/tests.rs
git commit -m "refactor(runtime): #1385 固化 sub run capability 偏序"
```

---

## Task 4：定义 Main Session shell 与 Main RuntimeContext assembler

**Issue 门禁：** Main 由 RunSpec 驡动装配；session shell 状态不进入 RuntimeContext。

**Files:**
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/client/trait_chat.rs`
- Modify: `agent/features/runtime/src/ports/legacy.rs`
- Modify: `agent/features/runtime/src/application/service.rs`

- [ ] **Step 1：写 Main shell 分类失败测试**

在 client 现有外置测试或 `from_args.rs` 现有测试中增加：

- bootstrap 后可取得 `MainSessionShell`；shell 持有 wiring/workspace/session query/model switch/prompt bootstrap。
- shell 不持 `RuntimeContext` 实例，因为 RuntimeContext 必须每个 Run 新建。
- 两次 Main Run assembler 调用产生不同 cancel token，但共享允许共享的父能力 Arc。
- model switch 后仅下一 Run assembler 使用新 binding，已构造 context 不变。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib main_session_shell -- --nocapture`

Expected: FAIL，当前仍以 `ChatRuntimeContext { resources, verbose, resume }` 承载所有资源。

- [ ] **Step 3：实现 shell 与 assembler 输入**

将 `RuntimeHandle.context` 替换为命名明确的 `MainSessionShell`。`ChatRuntimeContext` 若只剩启动参数则重命名为 `ChatRuntimeBootstrap { verbose, resume }`；若无消费方则删除，禁止与既有 `main_loop/request.rs::ChatLaunchOptions` 重名。RuntimeHandle 中 session shell 字段必须按 2.2 分类，禁止把 service 集合重新包装成 `Resources` 同义结构。

Main assembler 接口固定表达 RunSpec：

```rust
fn assemble_main_runtime_context(
    &self,
    spec: &RunSpec,
    bound: &context::BoundMainRun,
    input: Arc<dyn InputBuffer>,
    events: Arc<dyn EventSink>,
) -> Result<RuntimeContext, RuntimeContextAssemblyError>;
```

当前 Main input/event 生产类型若尚未实现骨架 trait，先在 adapters 层提供桥接；不得把 queue/sink 具体类型塞入 RuntimeContext。

- [ ] **Step 4：迁移 bootstrap**

`from_args.rs` 只装配 Main Session shell 父能力，不再创建单个跨 Run RuntimeContext。保留 `MainSessionWiring`、WorkspaceViews、provider factory、session management 等 session 状态在 RuntimeHandle。

- [ ] **Step 5：运行 GREEN**

Run: `cargo test -p runtime --lib main_session_shell -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --lib from_args -- --nocapture`

Expected: PASS。

- [ ] **Step 6：提交**

```bash
git add agent/features/runtime/src/application/client \
  agent/features/runtime/src/ports/legacy.rs \
  agent/features/runtime/src/application/service.rs
git commit -m "refactor(runtime): #1385 分离 main session shell 与 run 资源"
```

---

## Task 5：将 Main 生产路径切到 RuntimeContext

**Issue 门禁：** RuntimeContext 成为 Main 唯一 per-Run 活资源容器；ChatLoopContext 不复制 service 字段。

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_context.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/pre_compact_trigger_tests.rs`（其中直接构造 MainRunPort 的 fixture 改为注入 RuntimeContext）

- [ ] **Step 1：写 Main 装配失败测试**

新增/修改测试证明：

1. `main_run_assembly_uses_run_spec_main`：launcher 和 context assembler 收到同一个 `RunSpec::main()`。
2. `main_run_port_reads_live_contracts_from_runtime_context`：替换其中一个 fake policy/tool/hook 后，MainRunPort 行为由 RuntimeContext 中实例决定。
3. `chat_loop_context_contains_no_run_service_copies`：通过构造 API 编译约束，ChatLoopContext 只接收 `MainSessionShell`、launch input 和 `SessionQueryPort`，不再逐字段接收 binding/tool/policy/memory/task/hook/reasoning。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib main_run_assembly -- --nocapture`

Expected: FAIL，当前 `loop_runner.rs` 直接把 20+ service 字段写入 MainRunPort。

- [ ] **Step 3：在 Run 创建点装配 RuntimeContext**

在 `bind_main_run` 和 `RunConfigSnapshot::capture` 之后：

```rust
let spec = RunSpec::main();
let runtime_context = shell.assemble_main_runtime_context(
    &spec,
    &bound_main_run,
    run_input,
    run_events,
)?;
```

同一个 `spec` 同时传给 `RunLaunchInput`，禁止重新调用 `RunSpec::main()` 形成两份声明。

- [ ] **Step 4：MainRunPort 只持 RuntimeContext + 执行状态**

删除 MainRunPort 中已由 RuntimeContext 提供的 service 字段；保留 messages、Step ownership、ContextRequest/Window、turn usage、tool identity、continuation、terminal 等 #1397 才处理的执行状态，以及必要 session shell 借用。所有 Tool/Policy/Hook/Memory/Task/Reasoning/Provider 读取经 RuntimeContext accessor。同步更新 `pre_compact_trigger_tests.rs` 中直接构造 MainRunPort 的 fixture，改为构造 RuntimeContext，保留原有 compact/reflection 语义断言。

- [ ] **Step 5：运行 GREEN**

Run: `cargo test -p runtime --lib main_run_assembly -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --lib loop_runner_tests -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --test main_session_wiring_integration -- --nocapture`

Expected: PASS。

- [ ] **Step 6：提交**

```bash
git add agent/features/runtime/src/application/main_loop/looping \
  agent/features/runtime/tests/main_session_wiring_integration.rs
git commit -m "refactor(runtime): #1385 接通 main runtime context"
```

---

## Task 6：从父能力派生 Sub RuntimeContext

**Issue 门禁：** Sub 由父 RunSpec/RuntimeContext 收缩派生，不可扩权。

**Files:**
- Modify: `agent/features/runtime/src/application/subagent/runner.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests.rs`

- [ ] **Step 1：写 Sub L2 失败测试**

测试至少覆盖：

- `sub_context_derivation_uses_parent_cancel_child_scope`：取消父 scope 后子 token 取消，取消子不取消父。
- `sub_context_derivation_restricts_tool_catalog`：只使用 `sub-agent/sub-agent-restricted` snapshot，且不能注入 full snapshot。
- `sub_context_derivation_disables_memory_by_default`：默认为 NoOpMemory，不与父 Main memory Arc 相同。
- `sub_context_derivation_uses_isolated_context`：ContextPort 与父不相同。
- `sub_context_derivation_does_not_widen_policy_or_interaction`：policy 只能同一 Arc/更严；non-interactive Sub 不得接收 Main 的 `InteractionBridge` 或 SDK/TUI 交互通道。
- `sub_launcher_uses_derived_spec`：setup 产生的 derived spec 与 launcher 输入是同一值，不再在 `loop_run.rs` 重新 `RunSpec::sub()`。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p runtime --lib sub_context_derivation -- --nocapture`

Expected: FAIL；当前 setup 手工拼接资源，loop_run 再单独构造 RunSpec。

- [ ] **Step 3：实现唯一派生入口**

唯一 `derive_sub_run` helper 放在 `runner/setup.rs`，由 `CliAgentRunner::run_agent` 调用；`runner.rs` 只为 `CliAgentRunner` 增加所需父 RuntimeContext/source 字段，不复制派生规则。入口必须同时返回 spec/context/workspace child scope，避免三者分开决定：

```rust
pub struct DerivedSubRun {
    pub spec: RunSpec,
    pub context: RuntimeContext,
    pub workspace: project::WorkspaceViews,
}

fn derive_sub_run(
    parent_spec: &RunSpec,
    parent_context: &RuntimeContext,
    parent_workspace: &RuntimeWorkspaceAccess,
    request: &SubRunRequest,
) -> Result<DerivedSubRun, RuntimeContextAssemblyError>;
```

Workspace child scope 由 Project-owned `derive_isolated()` 产生，只作为 Composition/setup 结果传给 Tool/Context backing；不得存入 RuntimeContext。

- [ ] **Step 4：SubAgentRun 消费 RuntimeContext**

像 Main 一样删除已由 RuntimeContext 提供的 service 字段，但保留 #1397 的执行状态。`RunLaunchInput.spec` 使用 `DerivedSubRun.spec`，不允许二次构造。

- [ ] **Step 5：运行 GREEN**

Run: `cargo test -p runtime --lib sub_context_derivation -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --lib subagent::runner -- --nocapture`

Expected: PASS。

- [ ] **Step 6：提交**

```bash
git add agent/features/runtime/src/application/subagent
git commit -m "refactor(runtime): #1385 收缩派生 sub runtime context"
```

---

## Task 7：删除 RuntimeResources 与所有资源复制路径

**Issue 门禁：** RuntimeResources 删除；RuntimeContext 是唯一 per-Run 活资源容器。

**Files:**
- Delete: `agent/features/runtime/src/application/resources.rs`
- Modify: `agent/features/runtime/src/application.rs`
- Modify: `agent/features/runtime/src/ports/legacy.rs`
- Modify: `agent/features/runtime/src/application/service.rs`
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/client/trait_chat.rs`

- [ ] **Step 1：建立删除前失败证据**

Run: `rg -n "RuntimeResources|\.resources\.|application::resources" agent/features/runtime agent/composition`

Expected: 有匹配；保存输出到 PR Test plan 的 RED 证据摘要，不创建临时文件。

- [ ] **Step 2：删除旧容器和 module**

删除 `resources.rs` 和 `pub mod resources`。删除或重命名 `ChatRuntimeContext`，不得保留仅换名的 20 字段 resources bag。

- [ ] **Step 3：清理所有消费者**

SDK/TUI launch projection 从 Main Session shell 取 session 状态，从 per-Run assembler 取活契约；不能为了兼容再次复制一份 RuntimeContext 字段。

- [ ] **Step 4：验证零残留**

Run: `rg -n "RuntimeResources|\.resources\.|application::resources" agent/features/runtime agent/composition`

Expected: 无输出，exit 1。

Run: `cargo build -p runtime`

Expected: exit 0。

Run: `cargo build -p composition`

Expected: exit 0。

- [ ] **Step 5：提交**

```bash
git add -A agent/features/runtime agent/composition
git commit -m "refactor(runtime): #1385 删除 runtime resources 复制路径"
```

---

## Task 8：补 Composition → Main/Sub 装配契约

**Issue 门禁：** Main/Sub 每层装配契约通过；Composition scope 不泄漏进 RuntimeContext。

**Files:**
- Modify: `agent/composition/src/runtime.rs`
- Create: `agent/composition/tests/runtime_context_wiring.rs`
- Modify: `agent/features/runtime/tests/main_session_wiring_integration.rs`

- [ ] **Step 1：写 L3 失败契约测试**

通过 runtime 的公共 bootstrap/assembler 测试 factory 验证：

1. Composition 注入的 tool catalog/execution、policy、hook、task、provider factory 与 Main assembler 使用同一 backing。
2. 每个 Main Run 新建 cancellation scope。
3. Sub 从父能力派生 restricted catalog、isolated workspace/context、disabled memory。
4. `MainSessionWiring`、WorkspaceViews、ConfigQuery/Writer、SessionQueryPort 均不能从 RuntimeContext 公共 API 取得。

只允许暴露最小 `cfg(test)` factory；不得扩大生产 public API。

- [ ] **Step 2：运行 RED**

Run: `cargo test -p composition --test runtime_context_wiring -- --nocapture`

Expected: FAIL，新契约入口尚未接通。

- [ ] **Step 3：校正 Composition 注入**

`agent/composition/src/runtime.rs` 继续拥有 workspace/context/tool wiring 的组装职责，向 Runtime 传递 Main Session shell 父能力；不得直接构造 RunExecutionState，也不得把 Composition scope 传入 RuntimeContext。

- [ ] **Step 4：运行 GREEN**

Run: `cargo test -p composition --test runtime_context_wiring -- --nocapture`

Expected: PASS。

Run: `cargo test -p runtime --test main_session_wiring_integration -- --nocapture`

Expected: PASS。

- [ ] **Step 5：提交**

```bash
git add agent/composition/src/runtime.rs \
  agent/composition/tests/runtime_context_wiring.rs \
  agent/features/runtime/tests/main_session_wiring_integration.rs
git commit -m "test(composition): #1385 固化 main sub runtime context 装配"
```

---

## Task 9：执行 Issue 完成定义与架构门禁

**Files:**
- Modify only if validation reveals defects; do not weaken tests or guards.

- [ ] **Step 1：开发环境门禁**

Run: `scripts/setup-dev-env.sh --check`

Expected: Cargo 1.91+、hooksPath、build-dir 配置全部通过。

- [ ] **Step 2：格式化**

Run: `cargo fmt --all -- --check`

Expected: exit 0。若失败，运行 `cargo fmt --all` 后重新检查；不得手工调整格式。

- [ ] **Step 3：生产编译与定向测试**

Run: `cargo build -p runtime -p composition`

Expected: exit 0。

Run: `cargo test -p runtime --lib`

Expected: 0 failed。

Run: `cargo test -p runtime --tests`

Expected: 0 failed。

Run: `cargo test -p composition`

Expected: 0 failed。

- [ ] **Step 4：workspace 回归与 clippy**

Run: `cargo test --workspace`

Expected: 0 failed。

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: exit 0，无 warning。

- [ ] **Step 5：架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`

Expected: 所有 architecture guards（含 no-inline-tests）通过。

- [ ] **Step 6：静态完成定义核验**

Run: `rg -n "RuntimeResources|application::resources|WorkspacePort" agent/features/runtime agent/composition`

Expected: 无生产残留；若设计文档历史文字命中，逐条人工分类。

Run: `rg -n "list_models: Arc<|list_sessions: Arc<|list_reminders: Arc<|list_reflection_history: Arc<" agent/features/runtime/src`

Expected: 无旧查询闭包字段。

Run: `rg -n "MainSessionWiring|WorkspaceViews|SessionQueryPort|ConfigQuery|ConfigWriter" agent/features/runtime/src/application/runtime_context.rs`

Expected: 无输出。

- [ ] **Step 7：逐项回填 Issue/PR checklist**

创建 PR 前把本计划第 1 节九项逐条映射到：代码路径、测试名、命令输出。任何未完成项必须在 PR 中写明原因、影响和 follow-up Issue；无合理理由不得创建 PR或宣称完成。

- [ ] **Step 8：最终提交**

仅在验证引起必要修正时提交：

```bash
git add -A
git commit -m "refactor(runtime): #1385 完成 runtime context 生产接线"
```

---

## 3. 测试分层与证据矩阵

| 层级 | 证据 | 覆盖责任 |
|---|---|---|
| L0 | build、clippy、architecture guards、零残留 grep | RuntimeContext 禁止依赖、生产可达性、旧容器退役 |
| L1 | `agent_run` spec tests、cancellation tests | capability 偏序、父子 cancel 不变量 |
| L2 | runtime_context tests、main/sub assembler tests、loop tests | 单 crate 内 port 组合、资源 identity、Main/Sub 差异 |
| L3 | composition `runtime_context_wiring`、SessionQueryPort adapter contract | Composition→Runtime 与 Port/Adapter 边界 |
| L4 | `main_session_wiring_integration`、现有 subagent runner 场景 | Main session→Run、父 Run→Sub Run 完整旅程 |
| L5 | 不新增 | 本 Issue 不改变真实终端、网络或安装路径；L0-L4 已充分 |

## 4. 风险与停止条件

1. **发现 RuntimeContext 所需端口没有生产 adapter：** 不得用 fake 进入生产。优先使用当前真实 Published Language；若必须新增 adapter，先补契约测试。若会扩大到新的 BC ownership，停止并请求是否拆 Issue。
2. **发现 #1397 才能移除的执行状态阻塞接线：** 保留 fat adapter，通过 `&RuntimeContext` 消费资源，不提前提取 `RunExecutionState`。
3. **发现 Sub parent-mediated interaction 尚未完成：** 使用明确 unavailable/restricted adapter，不能共享 Main SDK/TUI waiter 扩权；在 PR 记录与 #1248/#1397 的关系。
4. **三次修正仍无法让同一装配测试通过：** 停止并重新审视 RuntimeContext 边界，不继续叠加兼容层。
5. **架构守卫失败：** 只修根因；不得跳过、删 guard 或扩大白名单。若 hook 本身阻断且不属 #1385，按 workflow 记录后询问用户。

## 5. PR 前最终完成定义

只有同时满足以下条件才可创建 PR：

- 本计划第 1 节所有 Issue 门禁均打勾或有书面不适用理由。
- `RuntimeResources`、旧四查询闭包、RuntimeContext WorkspacePort 均零生产引用。
- Main 和 Sub 的 RunSpec 与 RuntimeContext 由同一装配动作产生，不存在二次 `RunSpec::main/sub`。
- Main/Sub 每层均有相邻测试，且 Composition 场景测试证明组合成立。
- build、runtime/composition tests、workspace tests、clippy、architecture guards 均有本次新鲜通过证据。
- PR 使用 `.github/pull_request_template.md`，填写 `Refs #1385`、Summary、Breaking change、Test plan，并明确 #1397/#1399 out-of-scope。
- Issue 保持 OPEN，等待用户确认后关闭。
