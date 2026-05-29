/// core/mod.rs — 核心流程（指挥官）：Provider 端口定义与客户端编排
/// - provider：LlmProvider / StreamHandler / CallbackHandler 端口 trait
/// - client：统一 LLM 客户端，按配置编排具体 provider
/// - pool：客户端池
pub mod client;
pub mod pool;
pub mod provider;
