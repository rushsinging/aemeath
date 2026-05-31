//! Provider adapter newtypes shared across assembly code.
//!
//! The shared crate owns only the dependency-free wrapper type. Runtime-specific
//! port implementations are supplied by the runtime crate during the migration
//! window to avoid adding upstream crate dependencies to share.

use std::sync::Arc;

/// LLM client newtype adapter.
pub struct LlmClientAdapter<T>(pub Arc<T>);

impl<T> LlmClientAdapter<T> {
    pub fn new(client: Arc<T>) -> Self {
        Self(client)
    }
}
