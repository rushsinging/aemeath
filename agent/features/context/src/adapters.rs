use std::sync::Arc;

mod atomic_blob_session;
mod canonical_session;
pub(crate) mod compact_summary;
mod in_memory_session;
pub mod memory_injection;
pub mod prompt;
mod prompt_source;
pub(crate) mod session_legacy_workspace;
mod session_management;
mod session_resume;

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
pub use session_management::{delete as delete_session_entry, export as export_session_bytes};
pub use session_management::{
    import as import_session_bytes, list as list_session_entries,
    load_canonical as load_canonical_session_entry,
    update_metadata as update_session_metadata_entry,
};

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
