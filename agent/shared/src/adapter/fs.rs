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
mod tests {
    use super::*;

    #[test]
    fn test_fs_adapter_new_wraps_inner() {
        let adapter = FsAdapter::new("fs");

        assert_eq!(adapter.0, "fs");
    }
}
