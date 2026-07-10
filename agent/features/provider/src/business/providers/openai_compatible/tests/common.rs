use super::driver::{
    AgnesDriver, ChatApiDriver, DeepSeekDriver, LiteLlmDriver, MimoDriver, MinimaxDriver,
    OpenAiDriver, VolcengineDriver, ZhipuDriver,
};
use super::*;
use crate::core::client::OpenAIProviderConfig;
use crate::core::provider::LlmProvider;
use serde_json::json;

fn base_body() -> serde_json::Value {
    json!({"model":"test-model","messages":[],"max_tokens":10,"stream":true})
}

fn assert_no_reasoning_fields(body: &serde_json::Value) {
    assert!(body.get("reasoning").is_none());
    assert!(body.get("thinking").is_none());
    assert!(body.get("enable_thinking").is_none());
}
