//! 多来源模型配置
//!
//! ## 模块结构
//! - `types` — 配置数据结构（ModelsConfig, ResolvedModel, ProviderModelsConfig, ModelEntryConfig）
//! - `error` — 模型解析错误类型
//! - `resolve` — 模型解析与查找逻辑
//! - `reasoning` — reasoning_effort 校验与支持检测
//! - `deserialize` — ModelEntryConfig 自定义反序列化

mod deserialize;
mod error;
mod reasoning;
mod resolve;
mod types;

// 类型
pub use types::*;
// 错误类型
pub use error::ModelResolveError;
// reasoning 工具
pub use reasoning::{supports_reasoning_effort, validate_reasoning_effort};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
