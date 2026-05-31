//! MCP adapter newtypes shared across assembly code.
//!
//! The shared crate owns only dependency-free wrapper types. Runtime-specific
//! port implementations remain in feature crates to keep the shared kernel pure.

/// MCP client/manager newtype adapter.
pub struct McpAdapter<T>(pub T);

impl<T> McpAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_adapter_new_wraps_inner() {
        let adapter = McpAdapter::new("mcp");

        assert_eq!(adapter.0, "mcp");
    }
}
