//! Provider adapters 层共享生产常量（#1146 双轨归位）。

pub(crate) const LLM_API_ERROR_TARGET: &str = "aemeath:llm-api-error";
pub(crate) const PREVIEW_LIMIT: usize = 1_024;
pub(crate) const SOURCE_CHAIN_LIMIT: usize = 8;

pub(crate) const ERROR_BODY_LIMIT: usize = 16 * 1024;

pub(crate) const REQUEST_ID_HEADERS: [&str; 4] = [
    "request-id",
    "x-request-id",
    "anthropic-request-id",
    "openai-request-id",
];

pub(crate) const ANTHROPIC_TOOL_ALLOWED_KEYS: &[&str] = &[
    "name",
    "description",
    "input_schema",
    "cache_control",
    "type",
];
