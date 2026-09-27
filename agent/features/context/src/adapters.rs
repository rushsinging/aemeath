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
mod legacy_flat_session_migrator;
pub mod memory_injection;
pub mod prompt;
mod prompt_source;
pub(crate) mod session_legacy_workspace;
#[cfg(any(test, feature = "dev"))]
mod session_lifecycle;
mod session_resume;
mod skill_prompt_source;
mod tool_receipt_ledger;

pub(crate) use atomic_blob_session::AtomicBlobSessionStore;
pub use atomic_blob_session_management::AtomicBlobSessionManagement;
pub(crate) use canonical_session::{
    AcceptedInputWriter, AtomicBlobCanonicalSessionWriter, CanonicalSessionRepository,
    CanonicalSessionWriter, ToolReceiptWriter,
};
pub use canonical_session::{
    AtomicBlobAcceptedInputWriter, AtomicBlobToolReceiptWriter, NoOpCanonicalSessionWriter,
    ProductionMainContextFactory,
};
pub use dataset_session_management::DatasetSessionManagement;
pub(crate) use dataset_session_reader::DatasetSessionReader;
pub use dataset_session_writer::DatasetCanonicalSessionWriter;
pub(crate) use in_memory_session::InMemorySessionRepository;
pub use legacy_flat_session_migrator::migrate_flat_sessions_to_project_dirs;
pub(crate) use memory_injection::{CommittedMemoryRetrieveAdapter, NoOpContextMemorySource};
pub use prompt_source::BaselinePromptSource;
pub use session_legacy_workspace::decode as decode_session;
pub(crate) use session_legacy_workspace::LegacySessionDecoder;
#[cfg(any(test, feature = "dev"))]
pub(crate) use session_lifecycle::capture as capture_session_lifecycle;
#[cfg(any(test, feature = "dev"))]
pub(crate) use skill_prompt_source::skill_prompt_budget;
pub(crate) use skill_prompt_source::SkillPromptSource;
pub use skill_prompt_source::WorkspaceSkillQueryFactory;

pub fn wire_isolated_context(session_id: &str) -> Arc<dyn crate::ports::ContextPort> {
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
/// Unlike [`wire_isolated_context`], the prompt pipeline lists metadata through the
/// injected [`tools::published::skill::SkillCatalogPort`] and [`SkillQueryFactory`].
pub(crate) fn wire_isolated_context_with_skill(
    session_id: &str,
    catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
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

/// Build an isolated (in-memory) context whose skill queries resolve against
/// the given workspace read view.
///
/// Unlike [`wire_isolated_context_with_skill`], callers pass only the workspace
/// port; the concrete [`WorkspaceSkillQueryFactory`] construction stays inside
/// the Context adapters so consumers never assemble cross-BC concrete types.
pub fn wire_isolated_context_with_workspace_skills(
    session_id: &str,
    catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
    workspace: Arc<dyn project::WorkspaceReader>,
) -> Arc<dyn crate::ports::ContextPort> {
    wire_isolated_context_with_skill(
        session_id,
        catalog,
        Arc::new(WorkspaceSkillQueryFactory::new(workspace)),
    )
}
