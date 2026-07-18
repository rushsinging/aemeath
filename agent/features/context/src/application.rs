pub mod main_session;
mod service;
mod session_persistence;

#[cfg(any(test, feature = "dev"))]
pub use main_session::test_support;
pub use main_session::{
    wire_main_session, BoundMainRun, MainSessionDependencies, MainSessionError, MainSessionWiring,
    MainSessionWiringBuilder, OwnedSessionExclusivePermit, OwnedSessionSharedPermit,
    SessionProjectionParticipant, SessionSwitchClosed, SessionSwitchGate, SessionSwitchInProgress,
};
pub use service::ContextApplicationService;
pub use session_persistence::{SessionLoadError, SessionPersistenceService};
