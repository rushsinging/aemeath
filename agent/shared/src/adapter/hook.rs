//! Hook adapter newtypes shared across assembly code.
//!
//! The shared crate owns only the dependency-free wrapper type. Runtime-specific
//! port implementations are supplied by the runtime crate during the migration
//! window to avoid adding upstream crate dependencies to share.

/// Hook runner newtype adapter.
pub struct HookRunnerAdapter<T>(pub T);

impl<T> HookRunnerAdapter<T> {
    pub fn new(runner: T) -> Self {
        Self(runner)
    }
}
