//! `ModelDownloadService`：手动模型下载用例（`aemeath systemone download` 的核心编排）。
//!
//! 行为契约（fail-closed + 幂等）：
//! - 先查 [`ModelAssetPort`]：`Installed` → 幂等成功且零网络调用；
//!   `Invalid` → typed 失败且零网络调用、不覆盖已有内容；
//! - `Missing` → 创建暂存区 → 按 manifest 顺序逐资产抓取 → 安装端
//!   重新校验（长度 / SHA-256 / canonical manifest）并原子 no-replace 提交；
//! - 任何失败（抓取 / 长度 / SHA / 结构 / 安装）都不出现 final 半成品，
//!   失败路径 MUST 显式异步清理暂存区（RAII `Drop` 只是兜底）。
//!
//! application 层只依赖 ports：不读环境变量 / 配置 / 文件系统（IO 全经端口），
//! 不包含固定 URL；manifest 与存储、抓取、安装端口全部由构造器注入。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::ModelManifest;
use crate::ports::{
    ArtifactFetchErrorKind, ArtifactFetcherPort, InstalledAssets, InvalidAssetKind, ModelAssetPort,
    ModelAssetState, ModelInstallPortErrorKind, ModelInstallerPort, StagedInstallOutcome,
};

#[cfg(test)]
#[path = "download_model_tests.rs"]
mod tests;

/// 下载成功结果：本次安装落地，或幂等命中已有有效安装。
///
/// 两者都是命令成功（[`Self::exit_code`] = 0）；只携带 CLI 需要的
/// revision 与安装根路径，NEVER 泄漏暂存 / IO 细节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// 本次下载已原子安装到 `<install_root>`。
    Installed {
        /// 已安装的 engine revision。
        revision: String,
        /// 已安装 revision 的根目录。
        install_root: PathBuf,
    },
    /// 本地已有有效安装（构造期查询命中，或下载期间并发竞争者先落地）：
    /// 幂等成功，未发生覆盖。
    AlreadyInstalled {
        /// 已安装的 engine revision。
        revision: String,
        /// 已安装 revision 的根目录。
        install_root: PathBuf,
    },
}

impl DownloadOutcome {
    /// 已安装的 engine revision。
    pub fn revision(&self) -> &str {
        match self {
            Self::Installed { revision, .. } | Self::AlreadyInstalled { revision, .. } => revision,
        }
    }

    /// 已安装 revision 的根目录。
    pub fn install_root(&self) -> &Path {
        match self {
            Self::Installed { install_root, .. } | Self::AlreadyInstalled { install_root, .. } => {
                install_root
            }
        }
    }

    /// CLI 退出码映射：成功 / 幂等 → 0。
    pub fn exit_code(&self) -> i32 {
        0
    }

    /// 由安装端口返回的已校验安装组装 outcome（`already_installed` 区分两类成功）。
    fn from_installed_assets(installed: InstalledAssets, already_installed: bool) -> Self {
        let revision = installed.manifest().engine_revision.clone();
        let install_root = installed.install_root().to_path_buf();
        if already_installed {
            Self::AlreadyInstalled {
                revision,
                install_root,
            }
        } else {
            Self::Installed {
                revision,
                install_root,
            }
        }
    }
}

/// 下载失败类别：typed kind —— CLI 据此映射非零退出，NEVER 字符串匹配决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelDownloadErrorKind {
    /// 构造期 manifest 契约校验失败（结构错误 fail-closed，零 IO / 零网络）。
    ManifestRejected,
    /// 本地安装状态为 `Invalid`（已存在但校验失败：不覆盖、不联网）。
    InvalidInstallState(InvalidAssetKind),
    /// 暂存区创建失败。
    StagingUnavailable(ModelInstallPortErrorKind),
    /// 资产抓取失败（网络 / 来源 / 状态 / 长度 / 目标写入）。
    Fetch(ArtifactFetchErrorKind),
    /// 安装（含最终校验与原子提交）失败。
    Install(ModelInstallPortErrorKind),
}

impl std::fmt::Display for ModelDownloadErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestRejected => write!(formatter, "模型 manifest 契约校验失败"),
            Self::InvalidInstallState(invalid_kind) => {
                write!(formatter, "本地模型安装无效（{invalid_kind}）")
            }
            Self::StagingUnavailable(install_kind) => {
                write!(formatter, "暂存目录不可用（{install_kind}）")
            }
            Self::Fetch(fetch_kind) => write!(formatter, "模型资产下载失败（{fetch_kind}）"),
            Self::Install(install_kind) => write!(formatter, "模型安装失败（{install_kind}）"),
        }
    }
}

/// 下载失败（typed kind + 中文 detail）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownloadError {
    /// 失败类别。
    pub kind: ModelDownloadErrorKind,
    /// 中文失败原因（含底层 typed 错误详情）。
    pub detail: String,
}

impl ModelDownloadError {
    /// CLI 退出码映射：任何失败 → 1（非零）。
    pub fn exit_code(&self) -> i32 {
        1
    }
}

impl std::fmt::Display for ModelDownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}：{}", self.kind, self.detail)
    }
}

impl std::error::Error for ModelDownloadError {}

/// 手动模型下载编排：manifest + 资产状态 / 安装 / 抓取三个端口全部注入。
///
/// 构造期先校验 manifest 契约（结构错误 fail-closed，零 IO / 零网络）；
/// 生产固定 manifest 与 composition 装配由后续任务提供。
pub struct ModelDownloadService {
    /// 当前期望的固定 manifest（下载与安装共用同一份，单一期望真相）。
    expected_manifest: ModelManifest,
    /// 本地安装状态解析端口。
    asset_store: Arc<dyn ModelAssetPort>,
    /// 暂存 / 校验 / 原子提交端口。
    installer: Arc<dyn ModelInstallerPort>,
    /// 逐资产流式抓取端口。
    fetcher: Arc<dyn ArtifactFetcherPort>,
}

impl std::fmt::Debug for ModelDownloadService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelDownloadService")
            .field("engine_revision", &self.expected_manifest.engine_revision)
            .finish_non_exhaustive()
    }
}

impl ModelDownloadService {
    /// 装配下载服务：先全量校验 manifest 契约，通过才接受注入的三个端口。
    pub fn new(
        expected_manifest: ModelManifest,
        asset_store: Arc<dyn ModelAssetPort>,
        installer: Arc<dyn ModelInstallerPort>,
        fetcher: Arc<dyn ArtifactFetcherPort>,
    ) -> Result<Self, ModelDownloadError> {
        expected_manifest
            .validate()
            .map_err(|error| ModelDownloadError {
                kind: ModelDownloadErrorKind::ManifestRejected,
                detail: error.to_string(),
            })?;
        Ok(Self {
            expected_manifest,
            asset_store,
            installer,
            fetcher,
        })
    }

    /// 执行下载用例：命中有效缓存幂等返回；缺失则流式抓取全部资产并原子安装。
    pub async fn download(&self) -> Result<DownloadOutcome, ModelDownloadError> {
        match self.asset_store.installed_assets().await {
            ModelAssetState::Installed(installed) => {
                return Ok(DownloadOutcome::from_installed_assets(installed, true));
            }
            ModelAssetState::Invalid { kind, detail } => {
                return Err(ModelDownloadError {
                    kind: ModelDownloadErrorKind::InvalidInstallState(kind),
                    detail,
                });
            }
            ModelAssetState::Missing => {}
        }
        let staging =
            self.installer
                .create_staging()
                .await
                .map_err(|error| ModelDownloadError {
                    kind: ModelDownloadErrorKind::StagingUnavailable(error.kind),
                    detail: error.to_string(),
                })?;
        for asset in &self.expected_manifest.assets {
            let fetch_failure = match self
                .fetcher
                .fetch_asset_into_staging(staging.path(), asset)
                .await
            {
                Ok(()) => continue,
                Err(failure) => failure,
            };
            // 正常失败路径：显式异步清理暂存区（RAII `Drop` 只是兜底）。
            let mut detail = fetch_failure.to_string();
            if let Err(cleanup_failure) = self.installer.discard_staging(staging).await {
                detail.push_str(&format!("；暂存目录清理失败：{cleanup_failure}"));
            }
            return Err(ModelDownloadError {
                kind: ModelDownloadErrorKind::Fetch(fetch_failure.kind),
                detail,
            });
        }
        match self.installer.install_staged(staging).await {
            Ok(StagedInstallOutcome::Installed(installed)) => {
                Ok(DownloadOutcome::from_installed_assets(installed, false))
            }
            Ok(StagedInstallOutcome::AlreadyValid(installed)) => {
                Ok(DownloadOutcome::from_installed_assets(installed, true))
            }
            Err(error) => Err(ModelDownloadError {
                kind: ModelDownloadErrorKind::Install(error.kind),
                detail: error.to_string(),
            }),
        }
    }
}
