/// 评分服务不可用：连接失败 / 超时 / 5xx / 线格式 422 或响应 schema 非法。
///
/// 消费点据此静默回退原路径（词法/启发式）；单次失败不熔断，逐次回退。
#[derive(Debug, Clone, PartialEq)]
pub struct ScoringUnavailable {
    kind: UnavailableKind,
    detail: String,
}

/// 不可用原因分类，供日志与审计归因；消费点只按「不可用」统一回退。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableKind {
    /// 评分开关关闭（Null adapter 的固定应答，非故障）。
    Disabled,
    /// 连接失败（服务未启动、网络不可达）。
    Connect,
    /// 请求超时。
    Timeout,
    /// 服务端 5xx。
    Server,
    /// 请求被拒绝（422）或响应 schema 非法。
    Schema,
}

impl ScoringUnavailable {
    pub fn new(kind: UnavailableKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> UnavailableKind {
        self.kind
    }
}

impl std::fmt::Display for ScoringUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            UnavailableKind::Disabled => "开关关闭",
            UnavailableKind::Connect => "连接失败",
            UnavailableKind::Timeout => "请求超时",
            UnavailableKind::Server => "服务端错误",
            UnavailableKind::Schema => "线格式非法",
        };
        write!(formatter, "评分服务不可用（{kind}）：{}", self.detail)
    }
}

impl std::error::Error for ScoringUnavailable {}
