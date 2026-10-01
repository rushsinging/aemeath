//! Storage adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! persistence implementations remain in feature crates to keep the shared kernel pure.

/// Storage service newtype adapter.
pub struct StorageAdapter<T>(pub T);

impl<T> StorageAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
