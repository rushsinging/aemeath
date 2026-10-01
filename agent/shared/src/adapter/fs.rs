//! Filesystem adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! file I/O implementations remain in feature crates to keep the shared kernel pure.

/// Filesystem service newtype adapter.
pub struct FsAdapter<T>(pub T);

impl<T> FsAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
#[path = "fs_tests.rs"]
mod tests;
