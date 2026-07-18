//! Narrow tools-owned seam for obtaining the current Memory port.
//!
//! [`MemoryTool`](crate::adapters::memory_tool::MemoryTool) must not capture an
//! `Arc<dyn MemoryPort>` at bootstrap because resume swaps the committed Memory
//! under the same registry. Instead the tool holds an [`Arc<dyn MemoryPortSource>`]
//! and calls [`MemoryPortSource::current`] at execution time. Runtime/Composition
//! provides the implementation; the canonical production source delegates to
//! `MainSessionWiring::committed_memory()`.
//!
//! Tool execution always happens inside the Main bound shared lease, so
//! `current()` returns the port that is bound for that Run.

use std::sync::Arc;

/// Object-safe, sync factory that returns the currently committed Memory port.
///
/// Implementations **must** be cheap (single `Arc` clone) and never block on
/// async locks — production sources read a `parking_lot::RwLock`-guarded
/// `Arc<dyn MemoryPort>` under a read guard.
pub trait MemoryPortSource: Send + Sync {
    /// Returns the currently committed [`memory::MemoryPort`].
    fn current(&self) -> Arc<dyn memory::MemoryPort>;
}

#[cfg(test)]
pub use test_support::test_memory_source;

/// Test-only helpers for creating a [`MemoryPortSource`] backed by
/// [`memory::InMemoryMemory`].
#[cfg(test)]
pub mod test_support {
    use super::*;

    /// Creates a fresh empty [`memory::InMemoryMemory`] backed source for tests.
    pub fn test_memory_source() -> Arc<dyn MemoryPortSource> {
        struct TestSource;
        impl MemoryPortSource for TestSource {
            fn current(&self) -> Arc<dyn memory::MemoryPort> {
                Arc::new(
                    memory::InMemoryMemory::new(memory::MemoryPolicy::default())
                        .expect("valid default policy"),
                )
            }
        }
        Arc::new(TestSource)
    }
}
