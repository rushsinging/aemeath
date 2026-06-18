//! 版本检查与自动更新 SDK trait + 公共类型。
//!
//! CLI 通过此 trait 调用更新能力，不直接依赖 `update` crate。

use async_trait::async_trait;

use crate::SdkError;

/// 版本检查结果。
#[derive(Debug, Clone)]
pub struct VersionCheck {
    /// 当前安装的版本。
    pub current_version: String,
    /// 最新发布版本。
    pub latest_version: String,
    /// 是否有可用更新。
    pub is_update_available: bool,
    /// Release 页面 URL。
    pub release_url: String,
    /// Release notes（可选）。
    pub release_notes: Option<String>,
}

/// 更新执行结果。
#[derive(Debug, Clone)]
pub enum UpdateResult {
    /// 已是最新版本。
    UpToDate { version: String },
    /// 更新成功。
    Updated { from: String, to: String },
    /// 仅检查未更新（`--check` 模式）。
    CheckOnly(VersionCheck),
}

/// 版本检查与自动更新服务。
#[async_trait]
pub trait UpdateService: Send + Sync + 'static {
    /// 检查最新版本（Quiet 模式用，带 24h 缓存）。
    async fn check_latest(&self) -> Result<VersionCheck, SdkError>;

    /// 强制检查（忽略缓存，用于 TUI 启动 + `aemeath update --check`）。
    async fn force_check(&self) -> Result<VersionCheck, SdkError>;

    /// 执行更新：下载 → 校验 → 原子替换。
    async fn perform_update(&self) -> Result<UpdateResult, SdkError>;
}
