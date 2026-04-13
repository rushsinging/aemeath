//! LLM Providers

pub mod anthropic;
pub mod ollama;
pub mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai_compatible::OpenAICompatibleProvider;