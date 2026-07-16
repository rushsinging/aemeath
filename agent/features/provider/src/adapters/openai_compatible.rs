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
pub(crate) mod reasoning_normalizer;
mod request_body;
mod responses;
mod responses_stream;
mod stream;
mod usage;

#[cfg(test)]
mod tests;

pub(crate) use driver::effort_from_thinking_tokens;
pub use provider::OpenAICompatibleProvider;
pub use reasoning::ReasoningConfig;
pub(crate) use responses_stream::parse_responses_stream;
pub(crate) use stream::parse_openai_stream;
