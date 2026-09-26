//! 跨 crate 边界错误分类标准（façade 边界收敛 SOP 的一部分）。
//!
//! 每个 feature crate 的边界错误（如 `AuditError`）使用与 [`ErrorCategory`]
//! 对齐的粗分类变体；细粒度内部错误（`pub(crate)`）在边界经 `From`
//! 折叠为分类 + 消息，永不越界。

/// 边界错误粗分类：所有 crate 的边界错误变体与之对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 底层存储/IO 失败。
    Storage,
    /// 调用方输入或状态非法。
    Invalid,
    /// 依赖不可用（worker 停止、锁占用、未接线等）。
    Unavailable,
}

impl ErrorCategory {
    /// 统一的粗分类名（日志与边界错误的稳定标签）。
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCategory::Storage => "storage",
            ErrorCategory::Invalid => "invalid",
            ErrorCategory::Unavailable => "unavailable",
        }
    }
}

/// 全仓唯一跨界错误类型（形态 B）：分类 + 消息 + 来源链。
///
/// - 控制流：`category()`（Storage/Invalid/Unavailable）；
/// - 呈现：`Display`（生成点定型的文案）；
/// - 诊断：`domain` 定位来源 crate，`source` 保留底层链（日志侧展开）。
#[derive(Debug, Clone)]
pub struct DomainError {
    domain: &'static str,
    category: ErrorCategory,
    message: String,
    source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
}

impl DomainError {
    /// 校验/输入/状态类错误（调用方可修正后重试）。
    pub fn invalid(domain: &'static str, message: impl Into<String>) -> Self {
        Self {
            domain,
            category: ErrorCategory::Invalid,
            message: message.into(),
            source: None,
        }
    }

    /// 底层存储/IO/git 失败。
    pub fn storage(domain: &'static str, message: impl Into<String>) -> Self {
        Self {
            domain,
            category: ErrorCategory::Storage,
            message: message.into(),
            source: None,
        }
    }

    /// 依赖不可用（worker 停止、装配失败等）。
    pub fn unavailable(domain: &'static str, message: impl Into<String>) -> Self {
        Self {
            domain,
            category: ErrorCategory::Unavailable,
            message: message.into(),
            source: None,
        }
    }

    /// 附带底层错误链（诊断用；呈现只看 message）。
    pub fn with_source(
        mut self,
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        self.source = Some(source);
        self
    }

    /// 折叠层构造（各 crate 的 From impl 使用；消费方用三个具名构造器）。
    pub fn from_parts(
        domain: &'static str,
        category: ErrorCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            domain,
            category,
            message: message.into(),
            source: None,
        }
    }

    /// 控制流分类。
    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    /// 来源 crate 标签（诊断定位）。
    pub fn domain(&self) -> &'static str {
        self.domain
    }

    /// 呈现文案（测试断言与结构化消费）。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl PartialEq for DomainError {
    /// 等价性按 domain/category/message（source 为诊断链，不参与等价）。
    fn eq(&self, other: &Self) -> bool {
        self.domain == other.domain
            && self.category == other.category
            && self.message == other.message
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for DomainError {}

impl DomainError {
    /// 底层错误链（诊断侧展开；Arc 共享，无生命周期陷阱）。
    pub fn source_error(&self) -> Option<&(dyn std::error::Error + Send + Sync)> {
        self.source.as_deref()
    }

    /// source 链按具体内部错误类型下钻（crate 内测试断言细变体字段用）。
    pub fn source_downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        self.source
            .as_deref()
            .and_then(|error| error.downcast_ref::<T>())
    }
}
