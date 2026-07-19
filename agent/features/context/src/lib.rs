pub(crate) const LOG_TARGET: &str = "aemeath:context";

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

pub use adapters::{compose_session_task_capture, LegacyTaskCapture};

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, BoundMainRun, MainSessionDependencies, MainSessionError, MainSessionWiring,
    MainSessionWiringBuilder, OwnedSessionExclusivePermit, OwnedSessionSharedPermit,
    SessionProjectionParticipant, SessionSwitchClosed, SessionSwitchGate, SessionSwitchInProgress,
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
    pub use crate::adapters::session_search::search_sessions;
    pub use crate::adapters::session_storage::{
        delete_session, list_sessions, load_canonical_session, load_session, save_session,
        update_session_metadata, SessionLoadError,
    };
    pub use crate::domain::session::{
        extract_project_name, new_session_id, now_iso, validate_session_id, CanonicalSession,
        ChatChain, ChatSegment, PersistedWorkspaceContext, PersistedWorkspaceFrame, SegmentKind,
        Session, SessionFilter, SessionMetadata, SessionRestore, SnapshotState,
    };
}
