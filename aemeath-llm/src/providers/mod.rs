//! LLM Providers

pub mod anthropic;
pub mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use openai_compatible::OpenAICompatibleProvider;