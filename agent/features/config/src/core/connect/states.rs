//! Connect 向导的状态值对象：阶段、origin、session id、revision。
//!
//! ## 设计约束
//!
//! - `ConnectSessionId` **opaque**：`start_connect` 返回，`apply` / `cancel`
//!   接受；不可由外部构造（构造函数仅在 service 内部使用）；
//! - `ConnectRevision` 单调递增；每个 session 独立计数，从 `initial()` 起，
//!   每次成功 apply 增加 1；
//! - `ConnectOrigin` 区分主动 Connect 与首次聊天初始化；不影响状态转换路径
//!   但决定取消与回退策略；
//! - 所有字段在 `Debug` 输出中清晰可读，但不暴露敏感字符串。

use std::fmt;

use uuid::Uuid;

use crate::catalog::ProviderSource;

/// Connect 向导的当前阶段。
///
/// 阶段语义见 [`docs/design/02-modules/config/02-provider-catalog-and-connect.md §3.2`](../../../../../../docs/design/02-modules/config/02-provider-catalog-and-connect.md)。
///
/// - `SelectProvider`：列出 Catalog 内置 Provider；
/// - `ConfirmOverwrite`：若同名 Provider 已存在，要求用户确认覆盖；
/// - `EditEndpoint` / `EditCredential` / `EditUserAgent`：编辑 base URL /
///   凭证 / Provider UA；
/// - `SelectModel`：选择推荐模型或进入自定义模型编辑；
/// - `EditCustomModel`：填写自定义 model_id / context_window / max_tokens；
/// - `ChooseGlobalDefault`：选择是否将本次模型设为全局默认；
/// - `ChooseProbe`：选择跳过或执行连接探测；
/// - `Probing`：正在等待 `ProviderProbePort` 返回；
/// - `Review`：含 review 页面（无密钥明文）并提交；
/// - `Saving`：等待 `ConnectCommitPort` 返回；
/// - `Completed` / `Cancelled`：业务终态；终态后命令不再产生副作用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectStage {
    SelectProvider,
    ConfirmOverwrite,
    EditEndpoint,
    EditCredential,
    EditUserAgent,
    SelectModel,
    EditCustomModel,
    ChooseGlobalDefault,
    ChooseProbe,
    Probing,
    Review,
    Saving,
    Completed,
    Cancelled,
}

impl ConnectStage {
    /// 阶段是否是业务终态（Completed / Cancelled）。
    pub fn is_terminal(self) -> bool {
        matches!(self, ConnectStage::Completed | ConnectStage::Cancelled)
    }
}

/// Connect 向导的入口 origin。
///
/// 影响取消策略（主动 Connect 丢弃 draft 不修改配置；首次聊天初始化在
/// 用户取消时按 receipt 条件回滚），不影响状态机的转换路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectOrigin {
    /// 主动执行 `aemeath connect`。
    ExplicitCommand,
    /// 首次聊天初始化触发的 Connect。
    FirstChatBootstrap,
}

/// Connect session 的 opaque ID。
///
/// - 内部使用 [`Uuid::v7`] 保证时间序与唯一性；外部只能从 service 返回值
///   与 `View` 投影中读取，无法构造或推断；
/// - 不暴露内部 UUID；`Debug` 与 `Display` 显示脱敏的短前缀，足够识别但
///   不泄漏完整 ID（防日志关联）。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectSessionId(Uuid);

/// ConnectSessionId is intentionally opaque; the only public re-export
/// `lib.rs` provides is `ConnectSessionId` itself, not its inner UUID.
/// The `as_uuid()` accessor is reserved for internal logging and tests.
impl ConnectSessionId {
    /// 创建一个新的 session id。**仅允许在 service 内部调用**——生产 API
    /// 不导出；测试辅助通过 service 申请。
    pub(crate) fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// 在 Config ↔ SDK ACL 边界编码完整 opaque identity。
    pub fn to_transport_string(self) -> String {
        self.0.to_string()
    }

    /// 从 SDK ACL 输入恢复 identity；拒绝非 UUIDv7 值。
    pub fn from_transport_str(value: &str) -> Result<Self, String> {
        let parsed = Uuid::parse_str(value).map_err(|_| "Connect session id 无效".to_string())?;
        if parsed.get_version_num() != 7 {
            return Err("Connect session id 必须是 UUIDv7".to_string());
        }
        Ok(Self(parsed))
    }
}

impl fmt::Debug for ConnectSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 短前缀 8 字符，足以在日志中分辨不同 session 且不暴露完整 ID。
        let bytes = self.0.as_bytes();
        let mut short = [0u8; 8];
        for (i, item) in short.iter_mut().enumerate() {
            *item = bytes[i];
        }
        let hex = hex_encode(&short);
        write!(formatter, "ConnectSessionId({hex})")
    }
}

impl fmt::Display for ConnectSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

/// Connect session 的 revision。
///
/// 每次成功 `apply` 增加 1；用作 optimistic-concurrency 标记。任何对老
/// revision 的指令都返回 typed `StaleRevision`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectRevision(u64);

/// `from_raw` / `into_raw` 是 service 在 revision 推进、stale 判断里需要
/// 的窄通道；公开 API 不导出。导出受 [`crate::connect`] 模块的门控。
impl ConnectRevision {
    /// 初始 revision。`start_connect` 创建的 session 即处于该 revision。
    pub const fn initial() -> Self {
        Self(0)
    }

    /// 从 SDK ACL 的传输值恢复 revision。
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// 返回 SDK ACL 使用的无损传输值。
    pub const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn bump(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Connect 入口创建时使用的辅助结构。当用户在 `SelectProvider` 阶段选择某
/// 个 source 且发现已存在同 source Provider 时，服务进入 `ConfirmOverwrite`
/// 并以本投影回显现有 Provider 的稳定字段。
///
/// **不包含 API key 明文**。`source_key` 是**用户配置中原有的字符串**，
/// 不强求与 Provider Catalog 固定 source 名称一致；遇到固定 source
/// 时 service 会经 `ProviderSource` 查 catalog 完成进一步校验。
#[derive(Debug, Clone)]
pub struct ExistingProviderSnapshot {
    pub source_key: String,
    pub driver: Option<DriverIdOrString>,
    pub base_url: String,
    pub api_key_status: ExistingCredentialStatus,
    pub model_id: Option<String>,
    pub context_window: Option<usize>,
    pub max_tokens: Option<u32>,
}

/// 现有 Provider 的 driver 投影。Catalog 必须先存在才能构造 connector；
/// 不存在时降级为字符串保留（兼容尚未迁移的 driver）。
#[derive(Debug, Clone)]
pub enum DriverIdOrString {
    Known(crate::catalog::DriverId),
    Unknown(String),
}

impl DriverIdOrString {
    pub fn as_str(&self) -> &str {
        match self {
            DriverIdOrString::Known(driver) => driver.as_str(),
            DriverIdOrString::Unknown(s) => s.as_str(),
        }
    }

    pub fn as_known(&self) -> Option<&crate::catalog::DriverId> {
        match self {
            DriverIdOrString::Known(driver) => Some(driver),
            DriverIdOrString::Unknown(_) => None,
        }
    }
}

/// 现有 Provider 的凭证投影：仅显示是否曾存在凭证，不暴露明文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingCredentialStatus {
    /// 存在凭证（值不可见）。
    Present,
    /// 用户从未配置过凭证。
    Absent,
}

impl ExistingProviderSnapshot {
    /// 从现有 [`share::config::ProviderModelsConfig`] 投影构造 snapshot。
    /// API key 字段被归一化为 [`ExistingCredentialStatus`]，明文永不出
    /// 现。`driver` 不在 Config schema 时降级为 `Unknown(_)`。
    pub fn from_provider_config(
        source: &str,
        base_url: &str,
        api_key: Option<&str>,
        driver: Option<&str>,
        model_id: &str,
        context_window: usize,
        max_tokens: u32,
    ) -> Self {
        let api_key_status = if api_key.is_some_and(|value| !value.is_empty()) {
            ExistingCredentialStatus::Present
        } else {
            ExistingCredentialStatus::Absent
        };
        let driver_known = driver
            .and_then(crate::catalog::find_by_driver)
            .map(|entry| entry.driver);
        let driver = match (driver_known, driver) {
            (Some(known), _) => Some(DriverIdOrString::Known(known)),
            (None, Some(raw)) => Some(DriverIdOrString::Unknown(raw.to_string())),
            (None, None) => None,
        };
        let model_id_opt = if model_id.is_empty() {
            None
        } else {
            Some(model_id.to_string())
        };
        Self {
            source_key: source.to_string(),
            driver,
            base_url: base_url.to_string(),
            api_key_status,
            model_id: model_id_opt,
            context_window: (context_window > 0).then_some(context_window),
            max_tokens: (max_tokens > 0).then_some(max_tokens),
        }
    }

    /// 在 catalog 中是否存在该 source；存在时返回对应 [`ProviderSource`]，
    /// 否则返回 `None`。**所有 session 构建路径**应优先调用此函数以保
    /// 证 source 一致性。
    pub fn catalog_source(&self) -> Option<ProviderSource> {
        crate::catalog::find_by_source(&self.source_key).map(|entry| entry.source)
    }
}
