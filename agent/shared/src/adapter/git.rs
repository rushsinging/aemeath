//! Git adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! git command execution remains in feature crates to keep the shared kernel pure.

/// Git service newtype adapter.
pub struct GitAdapter<T>(pub T);

impl<T> GitAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
