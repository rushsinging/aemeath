//! Telemetry adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! telemetry emission remains in feature crates to keep the shared kernel pure.

/// Telemetry service newtype adapter.
pub struct TelemetryAdapter<T>(pub T);

impl<T> TelemetryAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_adapter_new_wraps_inner() {
        let adapter = TelemetryAdapter::new("telemetry");

        assert_eq!(adapter.0, "telemetry");
    }
}
