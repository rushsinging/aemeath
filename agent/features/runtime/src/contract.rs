//! Published language for the runtime feature.
//!
//! Runtime does not define SDK-facing DTOs locally. It re-exports the SDK
//! published language used by CLI/composition callers.

pub use sdk::{
    AgentClient, ChangeSet, ChatEvent, ChatRequest, ChatStream, CostInfo, ProjectContext,
    SessionSnapshot, TaskSummary,
};
