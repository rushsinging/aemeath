//! LLM Providers

pub mod anthropic;
// Ollama provider 暂无构造点，整体标注 dead_code 保留逻辑（refs #61 D3）。
#[allow(dead_code)]
pub mod ollama;
pub mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use openai_compatible::OpenAICompatibleProvider;
// 说明（refs #61 D3）：OllamaProvider 当前无任何构造点（仅工厂 client.rs 未覆盖 Ollama 分支），
// 收窄 crate 可见性后该 re-export 成为未使用项，先移除导出以消除 warning，
// 模块本身保留以备后续接入；模块内孤儿项以 #[allow(dead_code)] 标注保留。
