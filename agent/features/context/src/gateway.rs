//! Context crate 的 gateway 层——跨 crate 函数 / trait re-export。

pub mod context_port {
    pub use crate::context_port::{CompactionUrgency, ContextPort, ContextPortError};
}

pub mod guidance {
    pub use crate::prompt::gateway::guidance::*;
}

pub mod skill {
    pub use crate::prompt::gateway::skill::*;
}

pub mod session {
    pub use crate::session::{
        delete_session, extract_project_name, list_sessions, load_session, new_session_id, now_iso,
        save_session, search_sessions, update_session_metadata, validate_session_id, ChatChain,
        ChatSegment, PersistedWorkspaceContext, PersistedWorkspaceFrame, SegmentKind, Session,
        SessionFilter, SessionLoadError, SessionMetadata, SessionRestore,
    };
}

pub mod compact {
    pub use crate::compact::*;
}
