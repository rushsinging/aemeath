//! compact LLM 摘要的内部错误（#1863 反转后专用）。

/// LLM 摘要失败：provider 调用错误或 compact 语义（空响应/JSON 无效）。
#[derive(Debug)]
pub(crate) enum CompactLlmError {
    Provider(provider::ProviderError),
    /// 空响应 / typed JSON 无效（compact 语义，触发重试或本地回退）。
    InvalidSummary(String),
}

impl CompactLlmError {
    /// 降级原因标签（quality 载荷展示用）。
    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            Self::Provider(_) => "Provider",
            Self::InvalidSummary(_) => "InvalidSummary",
        }
    }
}

impl std::fmt::Display for CompactLlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(error) => write!(f, "LLM 调用失败：{error}"),
            Self::InvalidSummary(message) => write!(f, "compact 输出无效：{message}"),
        }
    }
}
