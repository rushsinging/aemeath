//! Process adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! process execution remains in feature crates to keep the shared kernel pure.

/// Process runner newtype adapter.
pub struct ProcessAdapter<T>(pub T);

impl<T> ProcessAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
