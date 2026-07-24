pub(crate) const LOG_TARGET: &str = "aemeath:context";

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

pub use adapters::{isolated_context, isolated_context_with_skill};
#[cfg(any(test, feature = "dev"))]
pub use adapters::{NoOpCanonicalSessionWriter, ProductionMainContextFactory};
pub use domain::session::{
    SessionListEntry, SessionManagementError, SessionMetadataUpdate, SessionRestoreStep,
    SessionResumeProjection,
};
pub use ports::SessionManagementPort;

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, BoundMainRun, MainSessionDependencies, MainSessionError, MainSessionWiring,
    MainSessionWiringBuilder, OwnedSessionExclusivePermit, OwnedSessionSharedPermit,
    SessionSwitchClosed, SessionSwitchGate, SessionSwitchInProgress,
};

pub mod api {
    pub use crate::adapters::MemoryRetrieveAdapter;
    pub use crate::ports::MemoryMaterialization;
}

pub mod context_port {
    pub use crate::domain::*;
    pub use crate::ports::ContextPort;
}

pub mod compact {
    pub use crate::adapters::compact_summary::*;
    pub use crate::domain::compact::*;
    pub use crate::domain::{
        autocompact_threshold, effective_context_window, estimate_message_tokens,
        estimate_messages_tokens, estimate_tokens, estimate_tool_schemas_tokens,
    };
}

pub mod guidance {
    pub use crate::adapters::prompt::{
        assess_guidance, init_guidance_dir, resolve_guidance, resolve_guidance_async,
        universal_execution_discipline, GuidanceAssessment, InstructionsLoadedHook,
    };
}

pub mod session {
    pub use crate::domain::session::{
        CanonicalSession, PersistedWorkspaceContext, PersistedWorkspaceFrame, SessionMetadata,
        SnapshotState,
    };
}
