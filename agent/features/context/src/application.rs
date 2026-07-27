pub mod main_session;
#[cfg(test)]
pub(crate) mod performance;
mod service;
#[cfg(test)]
#[path = "application/service_tests.rs"]
mod service_tests;
mod session_persistence;

#[cfg(any(test, feature = "dev"))]
pub use main_session::test_support;
pub use main_session::{
    wire_main_session, BoundMainRun, MainSessionDependencies, MainSessionError, MainSessionWiring,
    MainSessionWiringBuilder, OwnedSessionExclusivePermit, OwnedSessionSharedPermit,
    SessionSwitchClosed, SessionSwitchGate, SessionSwitchInProgress,
};
pub use service::ContextApplicationService;
pub use session_persistence::{SessionLoadError, SessionPersistenceService};
