pub mod main_session;
#[cfg(test)]
pub(crate) mod performance;
pub(crate) mod reminder_pipeline;
#[cfg(test)]
#[path = "application/reminder_pipeline_tests.rs"]
mod reminder_pipeline_tests;
mod service;
#[cfg(test)]
#[path = "application/service_tests.rs"]
mod service_tests;
mod session_persistence;

#[cfg(any(test, feature = "dev"))]
pub use main_session::test_support;
#[cfg_attr(not(test), allow(unused_imports))]
pub use main_session::{
    wire_main_session, MainSessionDependencies, MainSessionWiring, MainSessionWiringBuilder,
    OwnedSessionSharedPermit,
};
pub(crate) use service::ContextApplicationService;
pub(crate) use session_persistence::{SessionLoadError, SessionPersistenceService};
