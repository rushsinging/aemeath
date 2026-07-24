//! OpenAI 兼容 provider 实现
//! 使用 OpenAIProviderConfig 替代旧 Provider enum

mod driver;
mod message_conversion;
#[cfg(test)]
mod message_conversion_tests;
mod message_helpers;
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

pub use provider::OpenAICompatibleProvider;
pub use reasoning::ReasoningConfig;
pub(crate) use responses_stream::parse_responses_stream;
pub(crate) use stream::parse_openai_stream;
pub(crate) use usage::{parse_chat_raw_usage, parse_responses_raw_usage};
