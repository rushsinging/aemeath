//! 模型解析错误类型

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResolveError {
    MissingSelection {
        available_sources: Vec<String>,
    },
    InvalidFormat {
        selection: String,
    },
    UnknownSource {
        source: String,
        available_sources: Vec<String>,
    },
    UnknownModel {
        source: String,
        query: String,
        available_models: Vec<String>,
    },
    UnknownDriver {
        source: String,
        driver: String,
    },
}

impl fmt::Display for ModelResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSelection { available_sources } => write!(
                f,
                "未指定模型。请使用 --model <来源>/<模型>。可用来源：\n  {}",
                available_sources.join("\n  ")
            ),
            Self::InvalidFormat { selection } => {
                write!(f, "模型选择 '{}' 格式无效，请使用 <来源>/<模型>", selection)
            }
            Self::UnknownSource {
                source,
                available_sources,
            } => write!(
                f,
                "未找到模型来源 '{}'。\n可用来源：\n  {}",
                source,
                available_sources.join("\n  ")
            ),
            Self::UnknownModel {
                source,
                query,
                available_models,
            } => write!(
                f,
                "来源 '{}' 中未找到模型 '{}'。\n可用模型：\n  {}",
                source,
                query,
                available_models.join("\n  ")
            ),
            Self::UnknownDriver { source, driver } => write!(
                f,
                "来源 '{}' 的 driver '{}' 不受支持。支持的 driver：anthropic, openai, zhipu, litellm, volcengine, ollama",
                source, driver
            ),
        }
    }
}

impl std::error::Error for ModelResolveError {}
