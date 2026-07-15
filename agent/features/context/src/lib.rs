/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
pub const LOG_TARGET: &str = "aemeath:context";

pub(crate) mod capabilities;
pub mod context_port;
pub mod contract;

pub mod compact {
    pub use crate::capabilities::compact::*;
}

pub mod guidance {
    pub use crate::capabilities::prompt::{
        init_guidance_dir, resolve_guidance, resolve_guidance_async,
        universal_execution_discipline, InstructionsLoadedHook,
    };
}

pub mod skill {
    pub use crate::capabilities::prompt::{
        builtin_commit_skill, load_all_skills, load_all_skills_cached, load_and_filter_skills,
        load_skills_from_dir, parse_skill, read_skill_content, Skill,
    };
}

pub mod session {
    pub use crate::capabilities::session::{
        delete_session, extract_project_name, list_sessions, load_session, new_session_id, now_iso,
        save_session, search_sessions, update_session_metadata, validate_session_id, ChatChain,
        ChatSegment, PersistedWorkspaceContext, PersistedWorkspaceFrame, SegmentKind, Session,
        SessionFilter, SessionLoadError, SessionMetadata, SessionRestore,
    };
}
