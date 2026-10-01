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
#[path = "telemetry_tests.rs"]
mod tests;
