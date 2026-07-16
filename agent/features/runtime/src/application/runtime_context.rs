//! RuntimeContext —— 装配的活资源容器。
//!
//! 对应设计：`docs/design/02-modules/runtime/01-domain-model.md` S6。
//!
//! 按 RunSpec 装配出的执行资源容器：运行时构造，注入 Loop Engine。
//! 不可序列化，不进 Run 聚合。
//!
//! #873 只定义结构和 accessor；实际装配（Composition Root）在 #950 实现，
//! legacy adapter 在 #874-#879 各 coordinator 切换时创建。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::ports::{
    ContextPort, EventSink, HookPort, InputBuffer, MemoryPort, PolicyPort, ProviderPort,
    ReasoningPort, TaskPort, ToolCatalogPort, ToolExecutionPort, UsageSink, WorkspacePort,
};

/// per-Run 协作式取消作用域；属于 RuntimeContext 活资源，不持久化。
///
/// 子 Run 从父作用域派生，父取消会同步传播到全部子 Run。
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

/// 按 RunSpec 装配出的执行资源容器。
///
/// 所有字段私有，仅暴露 `&dyn` accessor——消费方拿不到具体 Port 实现，
/// 无法绕过端口边界。
///
/// 装配规则由 `RunSpec` 驱动，Composition Root 提供具体 Port 实例。
/// 详见 `06-ports-and-adapters.md` §3。
pub struct RuntimeContext {
    // ── 出站 Port 活实例 ──
    context: Arc<dyn ContextPort>,
    provider: Arc<dyn ProviderPort>,
    tool_catalog: Arc<dyn ToolCatalogPort>,
    tool_execution: Arc<dyn ToolExecutionPort>,
    policy: Arc<dyn PolicyPort>,
    memory: Arc<dyn MemoryPort>,
    task: Arc<dyn TaskPort>,
    workspace: Arc<dyn WorkspacePort>,
    hooks: Arc<dyn HookPort>,
    reasoning: Arc<dyn ReasoningPort>,
    usage: Arc<dyn UsageSink>,

    // ── 入站 & 出站 ──
    input: Arc<dyn InputBuffer>,
    events: Arc<dyn EventSink>,

    // ── per-Run 取消作用域 ──
    cancel: RunCancellationScope,
}

impl RuntimeContext {
    /// 由 Composition Root（#950）或 legacy adapter（#874–#879）构造。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: Arc<dyn ContextPort>,
        provider: Arc<dyn ProviderPort>,
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        policy: Arc<dyn PolicyPort>,
        memory: Arc<dyn MemoryPort>,
        task: Arc<dyn TaskPort>,
        workspace: Arc<dyn WorkspacePort>,
        hooks: Arc<dyn HookPort>,
        reasoning: Arc<dyn ReasoningPort>,
        usage: Arc<dyn UsageSink>,
        input: Arc<dyn InputBuffer>,
        events: Arc<dyn EventSink>,
        cancel: RunCancellationScope,
    ) -> Self {
        Self {
            context,
            provider,
            tool_catalog,
            tool_execution,
            policy,
            memory,
            task,
            workspace,
            hooks,
            reasoning,
            usage,
            input,
            events,
            cancel,
        }
    }

    // ── 只读 accessor（返回 &dyn，不泄漏具体实现） ──

    pub fn context(&self) -> &dyn ContextPort {
        self.context.as_ref()
    }
    pub fn provider(&self) -> &dyn ProviderPort {
        self.provider.as_ref()
    }
    pub fn tool_catalog(&self) -> &dyn ToolCatalogPort {
        self.tool_catalog.as_ref()
    }
    pub fn tool_execution(&self) -> &dyn ToolExecutionPort {
        self.tool_execution.as_ref()
    }
    pub fn policy(&self) -> &dyn PolicyPort {
        self.policy.as_ref()
    }
    pub fn memory(&self) -> &dyn MemoryPort {
        self.memory.as_ref()
    }
    pub fn task(&self) -> &dyn TaskPort {
        self.task.as_ref()
    }
    pub fn workspace(&self) -> &dyn WorkspacePort {
        self.workspace.as_ref()
    }
    pub fn hooks(&self) -> &dyn HookPort {
        self.hooks.as_ref()
    }
    pub fn reasoning(&self) -> &dyn ReasoningPort {
        self.reasoning.as_ref()
    }
    pub fn usage(&self) -> &dyn UsageSink {
        self.usage.as_ref()
    }
    pub fn input(&self) -> &dyn InputBuffer {
        self.input.as_ref()
    }
    pub fn events(&self) -> &dyn EventSink {
        self.events.as_ref()
    }
    pub fn cancel(&self) -> &RunCancellationScope {
        &self.cancel
    }
}
