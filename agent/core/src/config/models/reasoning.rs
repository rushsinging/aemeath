//! Reasoning effort 校验与支持检测

/// Valid reasoning_effort values.
const VALID_REASONING_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

/// Validate a reasoning_effort value. Returns `Ok(())` if valid.
pub fn validate_reasoning_effort(effort: &str) -> Result<(), String> {
    if VALID_REASONING_EFFORTS.contains(&effort) {
        Ok(())
    } else {
        Err(format!(
            "Invalid reasoning_effort '{}'. Valid values: {}",
            effort,
            VALID_REASONING_EFFORTS.join(", ")
        ))
    }
}

/// Check whether a model id supports reasoning_effort (OpenAI GPT-5.x / o-series).
pub fn supports_reasoning_effort(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}
