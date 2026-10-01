//! Composition-owned wiring for the process-active Memory service.

use memory::api::{MemoryPort, NoOpMemory};
use std::{future::Future, sync::Arc};

/// The Memory behavior assigned to a derived Sub Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMode {
    /// The Sub Run has no Memory reads, writes, or reflection side effects.
    Disabled,
    /// The Sub Run shares the active Main Run Memory service.
    Shared,
}

/// Narrow, role-named views of one active Memory service.
///
/// All four fields point at the same allocation; the names document where the
/// handles are distributed without introducing wrapper services or reopening
/// the project dataset.
#[derive(Clone)]
pub struct MemoryViews {
    pub context: Arc<dyn MemoryPort>,
    pub tools: Arc<dyn MemoryPort>,
    pub runtime: Arc<dyn MemoryPort>,
    pub reflection: Arc<dyn MemoryPort>,
}

/// A Memory service opened outside the active-slot installation boundary.
///
/// Preparing a candidate is deliberately separate from installing it so the
/// session-switch owner can perform preparation before entering its own gate.
pub struct PreparedMemory<I> {
    identity: I,
    memory: Arc<dyn MemoryPort>,
}

impl<I> PreparedMemory<I> {
    pub fn new(identity: I, memory: Arc<dyn MemoryPort>) -> Self {
        Self { identity, memory }
    }

    pub fn identity(&self) -> &I {
        &self.identity
    }

    pub fn memory(&self) -> &Arc<dyn MemoryPort> {
        &self.memory
    }
}

/// Owns the process-active Main Run Memory service and its comparable identity.
pub struct ActiveMemoryWiring<I> {
    active: PreparedMemory<I>,
}

impl<I: Eq> ActiveMemoryWiring<I> {
    pub fn new(initial: PreparedMemory<I>) -> Self {
        Self { active: initial }
    }

    pub fn identity(&self) -> &I {
        self.active.identity()
    }

    /// Clone role-specific handles from the one active service.
    pub fn main_views(&self) -> MemoryViews {
        MemoryViews {
            context: Arc::clone(&self.active.memory),
            tools: Arc::clone(&self.active.memory),
            runtime: Arc::clone(&self.active.memory),
            reflection: Arc::clone(&self.active.memory),
        }
    }

    /// Prepare a candidate without changing the active slot.
    ///
    /// An equal identity reuses the active allocation and does not invoke the
    /// opener. A different identity is opened once and returned for a later
    /// explicit `install`.
    pub async fn prepare<F, Fut, E>(&self, identity: I, opener: F) -> Result<PreparedMemory<I>, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Arc<dyn MemoryPort>, E>>,
    {
        if identity == self.active.identity {
            return Ok(PreparedMemory::new(
                identity,
                Arc::clone(&self.active.memory),
            ));
        }

        let memory = opener().await?;
        Ok(PreparedMemory::new(identity, memory))
    }

    /// Unconditionally replace the active slot with a prepared candidate.
    pub fn install(&mut self, prepared: PreparedMemory<I>) {
        self.active = prepared;
    }

    /// Derive a Sub Run Memory handle without opening another service.
    pub fn derive_sub(&self, mode: MemoryMode) -> Arc<dyn MemoryPort> {
        match mode {
            MemoryMode::Disabled => Arc::new(NoOpMemory),
            MemoryMode::Shared => Arc::clone(&self.active.memory),
        }
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
