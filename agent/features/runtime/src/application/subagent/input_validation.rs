//! Compatibility seam for the legacy Runtime call path.
//!
//! Schema ownership and implementation live in the Tools BC. Runtime retains
//! only the phase-metadata peel call site until the execution cutover.

pub use tools::{
    format_tool_input_error, strip_runtime_meta, validate_tool_input, ToolInputMismatch,
    RUNTIME_META_KEYS,
};
