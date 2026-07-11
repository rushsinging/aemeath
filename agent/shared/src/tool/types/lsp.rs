//! Typed result for the `lsp` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `lsp` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LspResult {
    pub output: String,
}

/// Typed input for the `lsp` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LspInput {
    /// The LSP operation to perform (diagnostics, symbols)
    pub operation: String,
    /// Absolute path to the file
    #[serde(alias = "filePath")]
    pub file_path: String,
    /// Language hint (rust, typescript, python, go). Auto-detected from file extension if omitted.
    pub language: Option<String>,
}
