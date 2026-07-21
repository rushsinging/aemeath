use std::sync::Arc;

mod atomic_blob_session;
mod atomic_blob_session_management;
mod canonical_session;
pub(crate) mod compact_summary;
mod in_memory_session;
pub mod memory_injection;
pub mod prompt;
mod prompt_source;
pub(crate) mod session_legacy_workspace;
mod session_resume;
mod skill_prompt_source;

pub use atomic_blob_session::AtomicBlobSessionStore;
pub use atomic_blob_session_management::AtomicBlobSessionManagement;
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
/// Unlike [`isolated_context`], the prompt pipeline materializes available
/// skills via the injected [`tools::SkillMaterializationPort`] supplier and the
/// injected [`SkillQueryFactory`]. This is the construction used by Runtime for
/// sub-agent isolated contexts so that sub-runs inherit the configured skill set.
pub fn isolated_context_with_skill(
    session_id: &str,
    materializer: Arc<dyn tools::SkillMaterializationPort>,
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
        Arc::new(SkillPromptSource::new(materializer, query_factory)),
        Arc::new(NoOpContextMemorySource),
    ))
}
