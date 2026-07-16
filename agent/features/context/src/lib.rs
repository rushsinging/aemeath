/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
pub const LOG_TARGET: &str = "aemeath:context";

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

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
        init_guidance_dir, resolve_guidance, resolve_guidance_async,
        universal_execution_discipline, InstructionsLoadedHook,
    };
}

pub mod skill {
    pub use crate::adapters::prompt::{
        builtin_commit_skill, load_all_skills, load_all_skills_cached, load_and_filter_skills,
        load_skills_from_dir, parse_skill, read_skill_content, Skill,
    };
}

pub mod session {
    pub use crate::adapters::session_search::search_sessions;
    pub use crate::adapters::session_storage::{
        delete_session, list_sessions, load_session, save_session, update_session_metadata,
        SessionLoadError,
    };
    pub use crate::domain::session::{
        extract_project_name, new_session_id, now_iso, validate_session_id, ChatChain, ChatSegment,
        PersistedWorkspaceContext, PersistedWorkspaceFrame, SegmentKind, Session, SessionFilter,
        SessionMetadata, SessionRestore,
    };
}
