use std::sync::Arc;

mod accepted_input_ledger;
mod atomic_blob_session;
mod atomic_blob_session_management;
mod canonical_session;
pub(crate) mod compact_summary;
mod dataset_session_management;
mod dataset_session_reader;
mod dataset_session_writer;
mod in_memory_session;
pub mod memory_injection;
pub mod prompt;
mod prompt_source;
pub(crate) mod session_legacy_workspace;
#[cfg(any(test, feature = "dev"))]
mod session_lifecycle;
mod session_resume;
mod skill_prompt_source;
mod tool_receipt_ledger;

pub use atomic_blob_session::AtomicBlobSessionStore;
pub use atomic_blob_session_management::AtomicBlobSessionManagement;
pub use canonical_session::{
    AcceptedInputWriter, AtomicBlobAcceptedInputWriter, AtomicBlobCanonicalSessionWriter,
    AtomicBlobToolReceiptWriter, CanonicalSessionRepository, CanonicalSessionWriter,
    NoOpAcceptedInputWriter, NoOpCanonicalSessionWriter, NoOpToolReceiptWriter,
    ProductionMainContextFactory, SessionWriteScope, ToolReceiptWriter,
};
pub use dataset_session_management::DatasetSessionManagement;
pub use dataset_session_reader::{DatasetSessionReader, PreparedDatasetResume};
pub use dataset_session_writer::DatasetCanonicalSessionWriter;
pub use in_memory_session::InMemorySessionRepository;
pub use memory_injection::{
    CommittedMemoryRetrieveAdapter, MemoryRetrieveAdapter, NoOpContextMemorySource,
};
pub use prompt_source::BaselinePromptSource;
pub use session_legacy_workspace::{decode as decode_session, LegacySessionDecoder};
#[cfg(any(test, feature = "dev"))]
pub use session_lifecycle::{
    capture as capture_session_lifecycle, SessionGenerationTransition, SessionLifecycleSnapshot,
    SessionStructureSnapshot,
};
pub use skill_prompt_source::{skill_prompt_budget, SkillPromptSource, WorkspaceSkillQueryFactory};

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

/// Build an isolated (in-memory) context whose prompt source is the
/// skill-aware [`SkillPromptSource`].
///
/// Unlike [`isolated_context`], the prompt pipeline lists metadata through the
/// injected [`tools::SkillCatalogPort`] and [`SkillQueryFactory`].
pub fn isolated_context_with_skill(
    session_id: &str,
    catalog: Arc<dyn tools::SkillCatalogPort>,
    query_factory: Arc<dyn crate::ports::SkillQueryFactory>,
) -> Arc<dyn crate::ports::ContextPort> {
    let repository = Arc::new(InMemorySessionRepository::new());
    repository.seed(
        &crate::domain::SessionId::new(session_id),
        crate::domain::SessionRevision::new(0),
        Vec::new(),
        None,
    );
    Arc::new(crate::application::ContextApplicationService::new(
        repository,
        Arc::new(SkillPromptSource::new(catalog, query_factory)),
        Arc::new(NoOpContextMemorySource),
    ))
}
