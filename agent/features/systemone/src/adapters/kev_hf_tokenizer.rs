//! HfKevTokenizer：生产 tokenizer 实现（HF fast tokenizer，`tokenizer/tokenizer.json`）。
//!
//! 复刻 kev `model.py::user_tokens` 语义：调用方文本中的 `<|name|>` 形态
//! special marker 先改写为 `<¦name¦>`（U+00A6），保证选项边界不可伪造，
//! 再以 `add_special_tokens=false` 编码；分隔 token id 经词表查表解析。

use std::path::Path;
use std::str::FromStr;

use crate::adapters::kev_causal_row::{CausalRowBuildError, KevDelimiter, KevRowTokenizer};

/// tokenizer 加载失败（启动期 fail-closed，不构造 port）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenizerLoadError {
    /// tokenizer.json 读取失败（文件缺失 / 权限）。
    ReadFailed { detail: String },
    /// tokenizer.json 解析失败（格式非法）。
    ParseFailed { detail: String },
}

impl std::fmt::Display for TokenizerLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFailed { detail } => write!(formatter, "读取 tokenizer 资产失败：{detail}"),
            Self::ParseFailed { detail } => write!(formatter, "解析 tokenizer 资产失败：{detail}"),
        }
    }
}

impl std::error::Error for TokenizerLoadError {}

impl std::fmt::Debug for HfKevTokenizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HfKevTokenizer")
            .finish_non_exhaustive()
    }
}

/// 生产 row tokenizer：包装 `tokenizers::Tokenizer`。
pub(crate) struct HfKevTokenizer {
    tokenizer: tokenizers::Tokenizer,
}

impl HfKevTokenizer {
    /// 从 tokenizer.json 源文本构造。
    pub(crate) fn from_json(source: &str) -> Result<Self, TokenizerLoadError> {
        let tokenizer = tokenizers::Tokenizer::from_str(source).map_err(|error| {
            TokenizerLoadError::ParseFailed {
                detail: error.to_string(),
            }
        })?;
        Ok(Self { tokenizer })
    }

    /// 从已安装文件构造（`tokenizer/tokenizer.json`）。
    pub(crate) fn from_file(path: &Path) -> Result<Self, TokenizerLoadError> {
        let source =
            std::fs::read_to_string(path).map_err(|error| TokenizerLoadError::ReadFailed {
                detail: format!("{}（{}）", error, path.display()),
            })?;
        Self::from_json(&source)
    }
}

impl KevRowTokenizer for HfKevTokenizer {
    fn tokenize_user_text(&self, text: &str) -> Result<Vec<i32>, CausalRowBuildError> {
        let rewritten = rewrite_user_text_special_markers(text);
        let encoding = self
            .tokenizer
            .encode(rewritten.as_str(), false)
            .map_err(|error| CausalRowBuildError::TokenizeFailed {
                detail: error.to_string(),
            })?;
        Ok(encoding
            .get_ids()
            .iter()
            .map(|token_id| i32::try_from(*token_id).unwrap_or(i32::MAX))
            .collect())
    }

    fn delimiter_id(&self, delimiter: KevDelimiter) -> Result<i32, CausalRowBuildError> {
        let token_text = delimiter.token_text();
        self.tokenizer
            .token_to_id(token_text)
            .map(|token_id| i32::try_from(token_id).unwrap_or(i32::MAX))
            .ok_or(CausalRowBuildError::MissingDelimiter { token_text })
    }
}

/// kev `user_tokens` 的 marker 改写：`<|name|>`（name ∈ `[A-Za-z0-9_]+`）→ `<¦name¦>`。
///
/// 改写后 marker 永远不会命中 special token，选项边界对调用方文本不可伪造。
/// 无 regex 依赖的逐字节扫描（模式全为 ASCII）。
pub(crate) fn rewrite_user_text_special_markers(text: &str) -> String {
    const BROKEN_BAR: char = '¦';
    let bytes = text.as_bytes();
    let mut rewritten = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"<|") {
            if let Some(end) = marker_end(bytes, cursor) {
                let marker_name = text
                    .get(cursor + 2..end - 2)
                    .expect("marker_end 仅返回 ASCII marker 的字符边界");
                rewritten.push('<');
                rewritten.push(BROKEN_BAR);
                rewritten.push_str(marker_name);
                rewritten.push(BROKEN_BAR);
                rewritten.push('>');
                cursor = end;
                continue;
            }
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor 落在字符边界内");
        rewritten.push(character);
        cursor += character.len_utf8();
    }
    rewritten
}

/// 从 `<|` 起扫描至 `|>`，name 仅允许 `[A-Za-z0-9_]`；非法形态返回 None。
fn marker_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start + 2;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'|' if bytes.get(cursor + 1) == Some(&b'>') => {
                return if cursor > start + 2 {
                    Some(cursor + 2)
                } else {
                    None
                };
            }
            byte if byte.is_ascii_alphanumeric() || byte == b'_' => cursor += 1,
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
#[path = "kev_hf_tokenizer_tests.rs"]
mod tests;
