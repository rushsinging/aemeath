use std::sync::{Arc, Mutex};

use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, EventFuture, RuntimeStreamEvent,
};
use crate::ports::{
    ContextPort, PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderBuildSpec,
    ProviderError, ProviderFactory, ProviderPort,
};
use hook::{HookInvocation, HookOutcome, HookPort};
use tools::{
    SkillCatalogPort, SkillDescriptor, SkillQuery, ToolCatalogError, ToolCatalogPort,
    ToolCatalogSnapshot, ToolExecutionOutcome, ToolExecutionPort, ToolInvocation, ToolProfileName,
};

pub(crate) struct FakeContextPort;

#[async_trait::async_trait]
impl ContextPort for FakeContextPort {
    async fn build_window(
        &self,
        _request: &crate::ports::ContextRequest,
    ) -> Result<crate::ports::ContextWindow, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }

    async fn needs_compaction(
        &self,
        _request: &crate::ports::ContextRequest,
    ) -> Result<crate::ports::CompactionDecision, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }

    async fn compact(
        &self,
        _request: &crate::ports::CompactRequest,
    ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }

    async fn manual_compact(
        &self,
        _request: &crate::ports::ManualCompactRequest,
    ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }

    async fn clear_session(
        &self,
        _session_id: &crate::ports::SessionId,
    ) -> Result<(), crate::ports::ContextPortError> {
        Ok(())
    }

    async fn append_and_persist(
        &self,
        _append: &crate::ports::ContextAppend,
    ) -> Result<crate::ports::AppendReceipt, crate::ports::ContextAppendError> {
        Err(crate::ports::ContextAppendError::Storage("fake".into()))
    }
}

pub(crate) struct FakeProviderPort;

#[async_trait::async_trait]
impl ProviderPort for FakeProviderPort {
    fn capabilities(
        &self,
        model: &crate::ports::ModelId,
    ) -> Result<crate::ports::ModelCapability, ProviderError> {
        Ok(crate::ports::ModelCapability {
            model: model.clone(),
            supports_tools: true,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: crate::ports::ReasoningCapability::none(),
            context_limit: None,
            output_limit: None,
        })
    }

    async fn invoke(
        &self,
        _request: crate::ports::InvocationRequest,
        _cancellation: &dyn provider::CancellationSignal,
    ) -> Result<crate::ports::InvocationStream, ProviderError> {
        Err(ProviderError::cancelled())
    }
}

pub(crate) fn fake_provider_binding() -> Arc<ProviderBinding> {
    Arc::new(ProviderBinding {
        provider: Arc::new(FakeProviderPort),
        model: crate::ports::ModelId {
            provider: "test-provider".into(),
            model: "test-model".into(),
        },
        max_tokens: 8192,
        requested_reasoning: provider::ReasoningLevel::Medium,
        context_window: Some(128_000),
    })
}

pub(crate) struct FakeProviderFactory;

impl ProviderFactory for FakeProviderFactory {
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError> {
        Ok(ProviderBinding {
            provider: Arc::new(FakeProviderPort),
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

pub(crate) struct FakeToolCatalog;

impl ToolCatalogPort for FakeToolCatalog {
    fn snapshot(
        &self,
        scope: &tools::RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError> {
        Ok(ToolCatalogSnapshot::new(
            scope.clone(),
            profile.clone(),
            Vec::new(),
        ))
    }
}

pub(crate) struct FakeToolExecution;

#[async_trait::async_trait]
impl ToolExecutionPort for FakeToolExecution {
    async fn execute(
        &self,
        _invocation: ToolInvocation,
        _context: &tools::ToolExecutionContext,
    ) -> ToolExecutionOutcome {
        ToolExecutionOutcome::success_text("fake")
    }
}

pub(crate) struct FakePolicyPort;

impl PolicyPort for FakePolicyPort {
    fn evaluate(&self, _request: &PolicyRequest) -> PolicyDecision {
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    }
}

pub(crate) struct FakeReflectionHistory;

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for FakeReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for FakeReflectionHistory {
    async fn append(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::MemoryError> {
        Ok(())
    }

    async fn upsert(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::MemoryError> {
        Ok(())
    }
}

pub(crate) struct FakeHookPort;

#[async_trait::async_trait]
impl HookPort for FakeHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> HookOutcome {
        HookOutcome::proceed()
    }
}

#[derive(Default)]
pub(crate) struct FakeSkillCatalog;

impl SkillCatalogPort for FakeSkillCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        Vec::new()
    }
}

#[derive(Clone, Default)]
pub(crate) struct RecordingEventSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl RecordingEventSink {
    pub(crate) fn handle(&self) -> ChatEventSinkHandle {
        ChatEventSinkHandle::new(self.clone())
    }

    pub(crate) fn events(&self) -> Vec<RuntimeStreamEvent> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(|event| match event {
                RuntimeStreamEvent::SystemMessage(message) => {
                    RuntimeStreamEvent::SystemMessage(message.clone())
                }
                _ => panic!("fixture only records cloneable SystemMessage markers"),
            })
            .collect()
    }
}

impl ChatEventSink for RecordingEventSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(event);
    }
}
