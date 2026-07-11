//! 多来源模型配置
//!
//! ## 模块结构
//! - `types` — 配置数据结构（ModelsConfig, ResolvedModel, ProviderModelsConfig, ModelEntryConfig）
//! - `error` — 模型解析错误类型
//! - `resolve` — 模型解析与查找逻辑
//! - `deserialize` — ModelEntryConfig 自定义反序列化

mod deserialize;
mod error;
mod resolve;
mod runtime;
mod types;

// 类型
pub use types::*;
// 错误类型
pub use error::ModelResolveError;
pub use runtime::{
    MaxTokensSource, ResolvedRuntimeModel, RuntimeModelRequest, RuntimeModelResolutionError,
    RuntimeModelResolver, DEFAULT_MAX_TOKENS,
};

#[cfg(test)]
#[path = "models/tests.rs"]
mod tests;
