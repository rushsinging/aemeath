//! Production adapter namespace.
//!
//! Adapter implementations are assembled by composition/runtime bootstrap. The
//! concrete modules live here as the DDD target namespace, while implementations
//! that depend on upstream runtime ports remain conditionally documented for the
//! migration window.

pub mod fs;
pub mod git;
pub mod hook;
pub mod mcp;
pub mod process;
pub mod provider;
pub mod storage;
pub mod telemetry;
