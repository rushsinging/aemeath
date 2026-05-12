//! OpenAI 兼容 provider 实现
//! 使用 OpenAIProviderConfig 替代旧 Provider enum

mod driver;
mod message_conversion;
#[cfg(test)]
mod message_conversion_tests;
mod message_helpers;
mod non_stream;
mod provider;
mod reasoning;
mod request_body;
mod stream;

#[cfg(test)]
mod tests;

pub(crate) use stream::parse_openai_stream;
// Re-export driver types for tests and external use
pub use driver::{
    effort_from_thinking_tokens, ChatApiDriver as _, LiteLlmDriver, OpenAiDriver, ZhipuDriver,
};
pub use provider::OpenAICompatibleProvider;
pub use reasoning::ReasoningConfig;
