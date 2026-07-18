//! Thin re-export of the canonical typed result struct.
//!
//! The authoritative definition lives in `tools::types::web_fetch`
//! so that runtime, tools and TUI can all share the same shape
//! without inverting the DDD layering.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a (方案 D) for the rationale.
pub use tools::types::web_fetch::WebFetchResult;
