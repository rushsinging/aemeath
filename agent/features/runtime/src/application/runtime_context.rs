//! RuntimeContext —— per-Run 活契约容器。
//!
//! 对应设计：`docs/design/02-modules/runtime/01-domain-model.md` S6。
//!
//! 按 RunSpec 装配出的执行资源容器：运行时构造，注入 Loop Engine。
//! 不可序列化，不进 Run 聚合。
//!
//! #1385：冻结生产契约 —— 只持当前生产可用的 per-Run 活契约；
//! WorkspacePort / MainSessionWiring / SessionQueryPort / ConfigQuery / ConfigWriter 不进入。
//! Main 由 Composition 提供父能力装配；Sub 从父收缩派生。
//!
//! #1248 Task 3 refactor —— 生命周期拆分：
//! - [`RuntimeServices`]：跨 Run 稳定共享（tool/policy/reflection/task/hooks + 未来 adapter factories）。
//! - [`RunContextBindings`]：per-Run 构造输入（context/provider/interaction/memory/reasoning + I/O seams），
//!   不是 Snapshot（含活契约），装配后不可变。
//! - [`RuntimeContext`]：装配产物，私有字段 + 只读 accessor。

use std::sync::{Arc, Mutex};

use sdk::ChatInputEvent;
use tokio_util::sync::CancellationToken;

use crate::application::interaction::InteractionPort;
use crate::application::main_loop::looping::run_input_buffer::RunInputBuffer;
use crate::application::main_loop::ChatEventSinkHandle;
use crate::application::run_config::RunConfigSnapshot;
use crate::domain::agent_run::RunSpec;
use crate::ports::{ContextPort, PolicyPort, ProviderBinding};
use hook::HookPort;
use memory::api::{MemoryPort, ReflectionHistoryStore};
use task::TaskAccess;
use tools::{ToolCatalogPort, ToolExecutionPort};

/// per-Run 协作式取消作用域；属于 RuntimeContext 活资源，不持久化。
///
/// ## Cancellation propagation
///
/// | Operation              | Propagation          | Use case                  |
/// |------------------------|----------------------|---------------------------|
/// | [`clone()`]            | bidirectional shared | general sharing           |
/// | [`child_scope()`]      | parent → child only  | sub-run derivation        |
///
/// [`clone()`] creates a clone that shares the same [`CancellationToken`]
/// (Arc-based internally).  Cancelling either clone cancels both — symmetric.
///
/// [`child_scope()`] creates a one-way link: parent cancellation propagates
/// to children, but child cancellation does not propagate upward.
/// This is the intended method for [`derive_sub_run`] so a cancelled sub-run
/// does not affect the Main Run.
#[derive(Clone)]
pub struct RunCancellationScope {
    token: CancellationToken,
}

impl RunCancellationScope {
    /// 创建根作用域。
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// 从父作用域派生子作用域——父取消传播到子。
    pub fn child_scope(&self) -> Self {
        Self {
            token: self.token.child_token(),
        }
    }

    /// 获取底层 CancellationToken。
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Default for RunCancellationScope {
    fn default() -> Self {
        Self::new()
    }
}

struct ToolCancellationSignal(CancellationToken);

#[async_trait::async_trait]
impl tools::CancellationSignal for ToolCancellationSignal {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    async fn cancelled(&self) {
        self.0.cancelled().await
    }

    fn child_signal(&self) -> Arc<dyn tools::CancellationSignal> {
        Arc::new(Self(self.0.child_token()))
    }
}

pub(crate) fn tool_cancellation_signal(
    token: CancellationToken,
) -> Arc<dyn tools::CancellationSignal> {
    Arc::new(ToolCancellationSignal(token))
}

struct ToolProgressSink(tokio::sync::mpsc::Sender<tools::AgentProgressEvent>);

impl tools::ProgressSink for ToolProgressSink {
    fn emit(&self, event: tools::AgentProgressEvent) {
        let _ = self.0.try_send(event);
    }
}

pub(crate) fn tool_progress_sink(
    tx: tokio::sync::mpsc::Sender<tools::AgentProgressEvent>,
) -> Arc<dyn tools::ProgressSink> {
    Arc::new(ToolProgressSink(tx))
}

// ── I/O seams (#1385 Task 11) ──

/// Per-Run usage tracker — shares `last_api_total_tokens` across all
/// clones of the same Run so the loop engine and observers see a
/// consistent view.
///
/// ## Poison strategy
///
/// Uses `std::sync::RwLock` (not `tokio::sync::RwLock`) because all
/// critical sections are pointer swaps / integer writes, never cross
/// `.await`, and must be callable from both sync and async contexts.
///
/// All access points use [`PoisonError::into_inner`] to recover the
/// inner data when the lock is poisoned, instead of panicking.  A
/// permanent poison would make usage tracking unavailable for the
/// remainder of the Run.
#[derive(Clone)]
pub struct RunUsageTracker {
    last_api_total_tokens: Arc<std::sync::RwLock<Option<u64>>>,
}

impl RunUsageTracker {
    /// Create a new tracker with no recorded usage.
    pub fn new() -> Self {
        Self {
            last_api_total_tokens: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Record the latest API total tokens value.
    pub fn update(&self, tokens: u64) {
        match self.last_api_total_tokens.write() {
            Ok(mut guard) => *guard = Some(tokens),
            Err(poison) => *poison.into_inner() = Some(tokens),
        }
    }

    /// Return the most recently recorded token count, if any.
    pub fn get(&self) -> Option<u64> {
        match self.last_api_total_tokens.read() {
            Ok(guard) => *guard,
            Err(poison) => *poison.into_inner(),
        }
    }

    /// Reset usage to `None` (e.g. after compaction invalidates the count).
    pub fn reset(&self) {
        match self.last_api_total_tokens.write() {
            Ok(mut guard) => *guard = None,
            Err(poison) => *poison.into_inner() = None,
        }
    }
}

impl Default for RunUsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: inner RwLock is Send + Sync, Arc handles sharing.
// The type is Clone (via Arc), Send, and Sync.

/// Handle to the per-Run input buffer — exposes only the **push side**
/// of [`RunInputBuffer`]; drain / epoch methods remain on the owning
/// `MainRunPort` so per-step execution state never leaks into
/// [`RuntimeContext`].
///
/// The inner [`RunInputBuffer`] is guarded by a `std::sync::Mutex`
/// (not `tokio::sync::Mutex`) because all critical sections are short
/// and never cross `.await`.
#[derive(Clone)]
pub struct RunInputBufferHandle {
    inner: Arc<std::sync::Mutex<RunInputBuffer>>,
}

impl RunInputBufferHandle {
    /// Create a new handle wrapping a fresh `RunInputBuffer`.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(RunInputBuffer::new())),
        }
    }

    /// Push an event unconditionally into the buffer.
    pub fn push(&self, event: ChatInputEvent) {
        match self.inner.lock() {
            Ok(mut guard) => guard.push(event),
            Err(poison) => poison.into_inner().push(event),
        }
    }

    /// Push an event; if the buffer is sealed and the event is a
    /// `UserMessage`, returns the event to the caller instead of
    /// silently buffering it.
    pub fn push_or_reject(&self, event: ChatInputEvent) -> Option<ChatInputEvent> {
        match self.inner.lock() {
            Ok(mut guard) => guard.push_or_reject(event),
            Err(poison) => poison.into_inner().push_or_reject(event),
        }
    }

    /// Whether the buffer has been sealed.
    pub fn is_sealed(&self) -> bool {
        match self.inner.lock() {
            Ok(guard) => guard.is_sealed(),
            Err(poison) => poison.into_inner().is_sealed(),
        }
    }

    /// Crate-private closure access to the inner [`RunInputBuffer`].
    /// The closure runs synchronously; never hold any returned reference
    /// across an `.await` point.
    pub(crate) fn with_lock<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RunInputBuffer) -> R,
    {
        match self.inner.lock() {
            Ok(mut guard) => f(&mut guard),
            Err(poison) => f(&mut poison.into_inner()),
        }
    }
}

impl Default for RunInputBufferHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ── #1248 Task 3: 装配输入拆分 ──

/// 长生命周期共享端口与工厂——会话级作用域，跨 Run 共享。
///
/// 只持跨 Run 稳定的能力：tool catalog/execution/binding、policy、
/// reflection history、task、hooks，以及后续 adapter factories。
/// **不持** per-Run 已绑定的 context/provider/interaction/memory ——
/// 这些属于 [`RunContextBindings`]。
///
/// 由 Composition Root 创建一次，通过 [`RuntimeContextFactory`]
/// 注入每次 Run 装配。所有字段均为 `Arc` 共享，Clone 即浅拷贝。
#[derive(Clone)]
pub struct RuntimeServices {
    /// Tool BC 只读目录端口。
    pub tool_catalog: Arc<dyn ToolCatalogPort>,
    /// Tool BC 执行端口。
    pub tool_execution: Arc<dyn ToolExecutionPort>,
    /// Policy BC 出站端口。
    pub policy: Arc<dyn PolicyPort>,
    /// Reflection 历史存储（会话级）。
    pub reflection_history: Arc<dyn ReflectionHistoryStore>,
    /// Task BC 低权限访问端口（会话级）。
    pub task: Arc<dyn TaskAccess>,
    /// Hook BC 出站端口。
    pub hooks: Arc<dyn HookPort>,
}

/// per-Run 构造输入——每次 Run 装配时提供的活契约集合。
///
/// 不是 Snapshot（Snapshot 暗示静态快照，但此处持有 `Arc<dyn Trait>`
/// 活契约端口）。由调用方在装配点提供；[`RuntimeContextFactory::assemble`]
/// 根据 [`RunSpec`] 的 capability-semantic 字段决定是否覆写其中某些端口
///（如 interaction、reasoning）。
///
/// # 生命周期语义
///
/// | 字段 | 作用域 | 说明 |
/// |---|---|---|
/// | `context` | per-Run | Context BC 出站端口 |
/// | `provider` | per-Run | Provider 绑定（含 model 冻结属性） |
/// | `interaction` | per-Run | 交互桥（ask_user / permission gate） |
/// | `memory` | per-Run | Memory BC 出站端口 |
/// | `reasoning` | per-Run | Reasoning 端口——factory 按 spec 覆写 |
/// | `config` | per-Run | Run 级固定配置快照 |
/// | `cancel` | per-Run | 取消作用域 |
/// | `event_sink` | per-Run | 事件输出 sink |
/// | `usage` | per-Run | token 用量追踪 |
/// | `input` | per-Run | 输入缓冲 handle（推入侧） |
#[derive(Clone)]
pub struct RunContextBindings {
    /// Context Management BC 出站端口。
    pub context: Arc<dyn ContextPort>,
    /// Provider 绑定（含 `Arc<dyn ProviderPort>` 与调用冻结属性）。
    pub provider: Arc<ProviderBinding>,
    /// 交互桥（ask_user / permission gate）。
    pub interaction: Arc<dyn InteractionPort>,
    /// Memory BC 出站端口。
    pub memory: Arc<dyn MemoryPort>,
    /// Run 级固定配置快照。
    pub config: RunConfigSnapshot,
    /// per-Run 取消作用域。
    pub cancel: RunCancellationScope,
    /// 事件输出 sink。
    pub event_sink: ChatEventSinkHandle,
    /// per-Run token 用量追踪。
    pub usage: RunUsageTracker,
    /// per-Run 输入缓冲 handle（推入侧）。
    pub input: RunInputBufferHandle,
    /// 静态 reasoning level，随 Run 冻结。
    pub reasoning: Arc<Mutex<share::reasoning::ReasoningLevel>>,
    /// #1248 Task 3: per-Run tool_catalog override (e.g. restricted catalog for sub-runs).
    /// When `None`, the factory's session-level catalog is used.
    pub tool_catalog: Option<Arc<dyn ToolCatalogPort>>,
}

/// 按 RunSpec 装配出的执行资源容器。
///
/// 所有字段私有，仅暴露 accessor——消费方拿不到具体 Port 实现，
/// 无法绕过端口边界。
///
/// 装配规则由 `RunSpec` 驱动，Composition Root 提供具体 Port 实例。
/// 详见 `06-ports-and-adapters.md` §3。
///
/// #1385 变更：
/// - 移除 `WorkspacePort`（无生产实现，过期端口）。
/// - `task` 从旧空壳 `TaskPort` 校正为生产已使用的 `TaskAccess`。
/// - `provider` 收敛为 `ProviderBinding`（含 port + model 约束）。
/// - 新增 `InteractionBridge`、`ReflectionHistoryStore`、`ToolExecutionContextBindingPort`。
/// - 不含 `MainSessionWiring`、`WorkspaceViews`、`SessionQueryPort`、`ConfigQuery`/`ConfigWriter`。
///
/// ## Clone & cancellation
///
/// [`Clone`] on [`RuntimeContext`] shares all `Arc<dyn Trait>` port fields
/// and the [`RunCancellationScope`] (which delegates to [`CancellationToken::clone()`] —
/// Arc-based sharing).  A cloned [`RuntimeContext`] therefore shares the
/// same cancellation token as the original: cancelling one cancels both.
/// For sub-run isolation where parent→child propagation is desired without
/// child→parent backflow, use [`RunCancellationScope::child_scope()`] via
/// [`derive_sub_run`].
#[derive(Clone)]
pub struct RuntimeContext {
    context: Arc<dyn ContextPort>,
    provider: Arc<ProviderBinding>,
    tool_catalog: Arc<dyn ToolCatalogPort>,
    tool_execution: Arc<dyn ToolExecutionPort>,
    policy: Arc<dyn PolicyPort>,
    interaction: Arc<dyn InteractionPort>,
    memory: Arc<dyn MemoryPort>,
    reflection_history: Arc<dyn ReflectionHistoryStore>,
    task: Arc<dyn TaskAccess>,
    hooks: Arc<dyn HookPort>,
    reasoning: Arc<Mutex<share::reasoning::ReasoningLevel>>,
    config: RunConfigSnapshot,
    cancel: RunCancellationScope,
    /// 事件输出 sink。
    event_sink: ChatEventSinkHandle,
    /// per-Run token 用量追踪。
    usage: RunUsageTracker,
    /// per-Run 输入缓冲 handle（推入侧）。
    input: RunInputBufferHandle,
}

/// Token that gates [`RuntimeContext::new`] — only [`RuntimeContextFactory`]
/// can construct one, preventing sibling modules from bypassing the factory.
///
/// #1248 Task 3: `RuntimeContext::new` must go through the factory so that
/// assembly rules (interaction/hook/reasoning binding modes) are validated.
pub struct RuntimeContextAssemblyToken(());

impl RuntimeContextAssemblyToken {
    /// Construct a token.  Only [`RuntimeContextFactory`] calls this in
    /// production; test code uses [`RuntimeContextAssemblyToken::new_for_test`].
    pub(crate) fn new() -> Self {
        Self(())
    }
}

#[cfg(test)]
impl RuntimeContextAssemblyToken {
    /// Test-only constructor so factory tests can build parent
    /// [`RuntimeContext`] instances directly.  Production code must
    /// go through [`RuntimeContextFactory::assemble`].
    pub(crate) fn new_for_test() -> Self {
        Self(())
    }
}

impl RuntimeContext {
    /// 仅 [`RuntimeContextFactory`](super::runtime_context_factory::RuntimeContextFactory)
    /// 可构造；外部通过 factory 装配。`_token` 由 factory 内部提供，
    /// 阻止 sibling 模块直接调用此构造函数。
    pub fn new(
        services: RuntimeServices,
        bindings: RunContextBindings,
        _token: RuntimeContextAssemblyToken,
    ) -> Self {
        Self {
            context: bindings.context,
            provider: bindings.provider,
            tool_catalog: services.tool_catalog,
            tool_execution: services.tool_execution,
            policy: services.policy,
            interaction: bindings.interaction,
            memory: bindings.memory,
            reflection_history: services.reflection_history,
            task: services.task,
            hooks: services.hooks,
            reasoning: bindings.reasoning,
            config: bindings.config,
            cancel: bindings.cancel,
            event_sink: bindings.event_sink,
            usage: bindings.usage,
            input: bindings.input,
        }
    }

    // ── Arc clone accessor（RunLoop adapter 需要共享 ownership） ──

    /// Context Management BC 端口，`Arc` clone。
    pub fn context(&self) -> Arc<dyn ContextPort> {
        self.context.clone()
    }
    /// Provider 绑定，`Arc` clone。
    pub fn provider(&self) -> Arc<ProviderBinding> {
        self.provider.clone()
    }
    /// Tool 目录端口（生产 `tools::ToolCatalogPort`），`Arc` clone。
    pub fn tool_catalog(&self) -> Arc<dyn ToolCatalogPort> {
        self.tool_catalog.clone()
    }
    /// Tool 执行端口（生产 `tools::ToolExecutionPort`），`Arc` clone。
    pub fn tool_execution(&self) -> Arc<dyn ToolExecutionPort> {
        self.tool_execution.clone()
    }
    /// Policy 端口，`Arc` clone。
    pub fn policy(&self) -> Arc<dyn PolicyPort> {
        self.policy.clone()
    }
    /// 交互桥，`Arc` clone。
    pub fn interaction(&self) -> Arc<dyn InteractionPort> {
        self.interaction.clone()
    }
    /// Memory 端口，`Arc` clone。
    pub fn memory(&self) -> Arc<dyn MemoryPort> {
        self.memory.clone()
    }
    /// Reflection 历史存储，`Arc` clone。
    pub fn reflection_history(&self) -> Arc<dyn ReflectionHistoryStore> {
        self.reflection_history.clone()
    }
    /// Task 访问端口，`Arc` clone。
    pub fn task(&self) -> Arc<dyn TaskAccess> {
        self.task.clone()
    }
    /// Hook 端口，`Arc` clone。
    pub fn hooks(&self) -> Arc<dyn HookPort> {
        self.hooks.clone()
    }
    /// Reasoning 端口，`Arc` clone。
    pub fn reasoning(&self) -> Arc<Mutex<share::reasoning::ReasoningLevel>> {
        self.reasoning.clone()
    }

    // ── 借用 accessor（Clone 类型） ──

    /// Run 级配置快照。
    pub fn config(&self) -> &RunConfigSnapshot {
        &self.config
    }
    /// per-Run 取消作用域。
    pub fn cancel(&self) -> &RunCancellationScope {
        &self.cancel
    }

    // ── Reference accessors (#1385): zero-clone borrow for MainRunPort borrow sites ──

    /// Context port reference.
    pub fn context_ref(&self) -> &Arc<dyn ContextPort> {
        &self.context
    }
    /// Provider binding reference.
    pub fn provider_ref(&self) -> &Arc<ProviderBinding> {
        &self.provider
    }
    /// Tool catalog port reference.
    pub fn tool_catalog_ref(&self) -> &Arc<dyn ToolCatalogPort> {
        &self.tool_catalog
    }
    /// Tool execution port reference.
    pub fn tool_execution_ref(&self) -> &Arc<dyn ToolExecutionPort> {
        &self.tool_execution
    }
    /// Policy port reference.
    pub fn policy_ref(&self) -> &Arc<dyn PolicyPort> {
        &self.policy
    }
    /// Interaction port reference.
    pub fn interaction_ref(&self) -> &Arc<dyn InteractionPort> {
        &self.interaction
    }
    /// Memory port reference.
    pub fn memory_ref(&self) -> &Arc<dyn MemoryPort> {
        &self.memory
    }
    /// Reflection history store reference.
    pub fn reflection_history_ref(&self) -> &Arc<dyn ReflectionHistoryStore> {
        &self.reflection_history
    }
    /// Task access reference.
    pub fn task_ref(&self) -> &Arc<dyn TaskAccess> {
        &self.task
    }
    /// Hook port reference.
    pub fn hooks_ref(&self) -> &Arc<dyn HookPort> {
        &self.hooks
    }
    /// Reasoning port reference.
    pub fn reasoning_ref(&self) -> &Arc<Mutex<share::reasoning::ReasoningLevel>> {
        &self.reasoning
    }
    /// Run config snapshot reference.
    pub fn config_ref(&self) -> &RunConfigSnapshot {
        &self.config
    }
    /// Run cancellation scope reference.
    pub fn cancel_ref(&self) -> &RunCancellationScope {
        &self.cancel
    }

    // ── I/O seam accessors (#1385 Task 11) ──

    /// 事件输出 sink，`Clone`。
    pub fn event_sink(&self) -> ChatEventSinkHandle {
        self.event_sink.clone()
    }
    /// per-Run token 用量追踪，`Clone`。
    pub fn usage(&self) -> RunUsageTracker {
        self.usage.clone()
    }
    /// per-Run 输入缓冲 handle，`Clone`。
    pub fn input(&self) -> RunInputBufferHandle {
        self.input.clone()
    }
}

// ── Parent-Run context source for sub-agent derivation ──

/// A single-source-of-truth snapshot of the parent Run that is available
/// to sub-agent derivation.  Carries both the parent [`RunSpec`] and
/// [`RuntimeContext`] so that [`derive_sub_run`] never needs a hard-coded
/// fallback.
#[derive(Clone)]
pub struct ParentRunFrame {
    pub spec: RunSpec,
    pub context: Arc<RuntimeContext>,
}

/// RAII guard that clears the parent frame on drop — but ONLY if the
/// generation at install time still matches the source.  This prevents a
/// stale guard (from a cancelled/crashed run) from clearing a fresh frame
/// installed by a new Main Run.
///
/// #1385 Task 7: Single session has only one Main frame at a time (Agent tool
/// is non-recursive), so no stack is needed.  The Main loop holds the guard.
///
/// ## Poison resilience
///
/// If the underlying `RwLock` has been poisoned (e.g. a prior panic while
/// holding the write lock), [`Drop`] recovers the inner data via
/// [`PoisonError::into_inner`] rather than panicking.  A panic during
/// [`Drop`] would cause a double-panic / process abort, which is
/// unacceptable for a cleanup guard.
pub struct ParentRunFrameGuard {
    source: ParentRunContextSource,
    generation: u64,
}

impl Drop for ParentRunFrameGuard {
    fn drop(&mut self) {
        // Poison recovery: use into_inner() to get the guard even if the
        // lock is poisoned.  Never panic in Drop — a second panic is an abort.
        let mut inner = match self.source.inner.write() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        };
        if inner.generation == self.generation {
            inner.frame = None;
        }
    }
}

/// Per-source state: generation counter + optional parent frame.
#[derive(Default)]
struct ParentRunContextSourceInner {
    /// Monotonic generation counter (wrapping, never 0).
    generation: u64,
    frame: Option<Arc<ParentRunFrame>>,
}

/// Injectable shared cell that carries the current Main Run's [`ParentRunFrame`]
/// to the sub-agent runner for use by [`derive_sub_run`].
///
/// Written by the Main Run loop before tool execution via [`install`]; read by
/// each sub-agent run via [`get`].  The returned [`ParentRunFrameGuard`] RAII
/// guard clears the frame on drop — no manual `clear()` needed.
///
/// #1385 Task 7: Each source carries its own generation counter (no global
/// static).  Generation wraps on overflow but never hits 0, preventing
/// stale-guard / fresh-frame collisions.
///
/// ## Lock choice
///
/// Uses `std::sync::RwLock` (not `tokio::sync::RwLock`).  All critical
/// sections are short (pointer swaps + counter bumps), never cross `.await`,
/// and must be callable from both sync and async contexts.  `tokio::sync`
/// guards would also make [`Drop`] impossible — you cannot `.await` in
/// [`Drop`].
///
/// ## Poison strategy
///
/// All three access points ([`install`], [`get`], [`ParentRunFrameGuard::drop`])
/// use [`PoisonError::into_inner`] to recover the inner data when the lock is
/// poisoned, instead of panicking.  This keeps the source usable after a
/// panic — a permanent poison would make the sub-agent path unavailable for
/// the remainder of the session.
#[derive(Clone, Default)]
pub struct ParentRunContextSource {
    inner: Arc<std::sync::RwLock<ParentRunContextSourceInner>>,
}

impl ParentRunContextSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the parent frame for the duration of the current Main Run.
    /// Returns an RAII guard that clears only its own generation on drop.
    ///
    /// Each call bumps the per-source generation counter (wrapping, never 0),
    /// so a stale guard from a cancelled/panicked run will never clear a
    /// fresh frame.
    ///
    /// Recovers from a poisoned lock rather than panicking.
    pub fn install(&self, frame: Arc<ParentRunFrame>) -> ParentRunFrameGuard {
        let mut inner = match self.inner.write() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        };
        inner.generation = inner.generation.checked_add(1).unwrap_or(1);
        inner.frame = Some(frame);
        ParentRunFrameGuard {
            source: self.clone(),
            generation: inner.generation,
        }
    }

    /// Read the current parent frame (if set).  Sub-agent derivation MUST
    /// fail closed when this returns `None` — no fallback to `RunSpec::main()`.
    ///
    /// Recovers from a poisoned lock rather than panicking.
    pub fn get(&self) -> Option<Arc<ParentRunFrame>> {
        match self.inner.read() {
            Ok(guard) => guard.frame.clone(),
            Err(poison) => poison.into_inner().frame.clone(),
        }
    }
}

#[cfg(test)]
#[path = "runtime_context_tests.rs"]
mod tests;
