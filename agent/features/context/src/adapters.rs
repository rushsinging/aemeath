mod atomic_blob_session;
pub(crate) mod compact_summary;
mod in_memory_session;
pub mod memory_injection;
pub mod prompt;
pub(crate) mod session_legacy_workspace;
pub(crate) mod session_search;
pub(crate) mod session_storage;
mod task_persistence;

pub use atomic_blob_session::AtomicBlobSessionStore;
pub use in_memory_session::InMemorySessionRepository;
pub use memory_injection::{MemoryRetrieveAdapter, NoOpContextMemorySource};
pub use session_legacy_workspace::{decode as decode_session, LegacySessionDecoder};
pub use task_persistence::{compose_session_task_capture, LegacyTaskCapture};

#[cfg(test)]
#[path = "adapters/task_persistence_tests.rs"]
mod task_persistence_tests;
