//! Shared test helpers for the OpenAI compatible adapter test modules.
//!
//! Kept under `pub(crate)` so sibling test sub-modules (`reasoning`,
//! `clamp_effort`, `provider_config`) can reach them via `super::common::*`.

use serde_json::json;

pub(crate) fn base_body() -> serde_json::Value {
    json!({"model":"test-model","messages":[],"max_tokens":10,"stream":true})
}

pub(crate) fn assert_no_reasoning_fields(body: &serde_json::Value) {
    assert!(body.get("reasoning").is_none());
    assert!(body.get("thinking").is_none());
    assert!(body.get("enable_thinking").is_none());
}
