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
mod tests {
    use super::*;

    #[test]
    fn test_storage_adapter_new_wraps_inner() {
        let adapter = StorageAdapter::new("storage");

        assert_eq!(adapter.0, "storage");
    }
}
