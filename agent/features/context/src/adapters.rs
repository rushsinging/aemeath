use std::sync::Arc;

mod atomic_blob_session;
mod canonical_session;
pub(crate) mod compact_summary;
mod in_memory_session;
pub mod memory_injection;
pub mod prompt;
mod prompt_source;
pub(crate) mod session_legacy_workspace;
pub(crate) mod session_search;
pub(crate) mod session_storage;
mod task_persistence;

pub use atomic_blob_session::AtomicBlobSessionStore;
pub use canonical_session::{
    AtomicBlobCanonicalSessionWriter, CanonicalSessionRepository, CanonicalSessionWriter,
    NoOpCanonicalSessionWriter, ProductionMainContextFactory,
};
pub use in_memory_session::InMemorySessionRepository;
pub use memory_injection::{
    CommittedMemoryRetrieveAdapter, MemoryRetrieveAdapter, NoOpContextMemorySource,
};
pub use prompt_source::BaselinePromptSource;
pub use session_legacy_workspace::{decode as decode_session, LegacySessionDecoder};
pub use task_persistence::{compose_session_task_capture, LegacyTaskCapture};

pub fn isolated_context(session_id: &str) -> Arc<dyn crate::ports::ContextPort> {
    let repository = Arc::new(InMemorySessionRepository::new());
    repository.seed(
        &crate::domain::SessionId::new(session_id),
        crate::domain::SessionRevision::new(0),
        Vec::new(),
        None,
    );
    Arc::new(crate::application::ContextApplicationService::new(
        repository,
        Arc::new(BaselinePromptSource),
        Arc::new(NoOpContextMemorySource),
    ))
}

#[cfg(test)]
#[path = "adapters/task_persistence_tests.rs"]
mod task_persistence_tests;
