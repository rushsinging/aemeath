//! 生产 embedded 评分装配：`wire_embedded_scoring`（设计 §4.1 唯一生产入口）。
//!
//! 装配链 `LocalModelAssetStore（三态资产解析）→ EmbeddedScoringAdapter（llama
//! worker）→ CalibratedScoringAdapter（温度校准）→ AuditedScoringAdapter（审计）`。
//!
//! 契约：
//! - **异步边界**：`installed_assets()` 与 `start()` 均为 await；文件 IO 已在
//!   adapter 内部的 `spawn_blocking` 中完成，本模块 NEVER 同步阻塞运行时线程。
//! - **fail-closed**：manifest 契约、资产缺失 / 损坏、tokenizer / PointerHead /
//!   worker 任一失败都返回 typed [`EmbeddedScoringWiringError`]，**NEVER** 构造
//!   port、**NEVER** 下载、**NEVER** 回退 HTTP。
//! - **feature 门控**：默认构建（feature `embedded` 关闭）返回 typed
//!   [`EmbeddedScoringWiringError::EmbeddedUnavailable`]，零 IO、不链接 llama.cpp。
//! - **审计 revision** 来自 `manifest.engine_revision`（单一真相），NEVER 从
//!   文件名或环境推断。
//!
//! 根目录与 scoring 目录全部由 composition 注入，本模块 **NEVER** 读取环境变量、
//! 也 **NEVER** 使用进程 cwd。

use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::ModelManifest;
use crate::ports::{InvalidAssetKind, ScoringPort};

#[cfg(any(feature = "embedded", test))]
use std::path::Path;

#[cfg(any(feature = "embedded", test))]
use crate::adapters::audited::AuditedScoringAdapter;
#[cfg(any(feature = "embedded", test))]
use crate::adapters::calibrated::CalibratedScoringAdapter;
#[cfg(any(feature = "embedded", test))]
use crate::adapters::calibration_store::CalibrationStore;
#[cfg(feature = "embedded")]
use crate::adapters::embedded::EmbeddedScoringAdapter;
#[cfg(feature = "embedded")]
use crate::adapters::model_assets::LocalModelAssetStore;
#[cfg(feature = "embedded")]
use crate::ports::ModelAssetPort;

/// 生产 embedded 装配失败（typed 分类 + 中文 Display）。
///
/// composition 直接映射为启动结果 variant，**NEVER** 折成字符串后靠猜测分类。
#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddedScoringWiringError {
    /// 本次构建未启用 feature `embedded`（默认 workspace 不链接 llama.cpp）。
    EmbeddedUnavailable,
    /// 注入的发行 manifest 契约非法（零 IO 拒绝）。
    ManifestInvalid {
        /// 中文失败原因。
        detail: String,
    },
    /// 本地模型资产未安装（提示执行 `aemeath systemone download`）。
    ModelMissing,
    /// 已安装但校验失败（损坏 / 不可读 / 不支持）。
    InvalidAssets {
        /// 失败类别。
        kind: InvalidAssetKind,
        /// 中文失败原因。
        detail: String,
    },
    /// manifest 资产角色契约错误（model.gguf / pointer_head / tokenizer 布局）。
    AssetContract {
        /// 中文失败原因。
        detail: String,
    },
    /// tokenizer 资产加载失败。
    TokenizerLoadFailed {
        /// 中文失败原因。
        detail: String,
    },
    /// PointerHead 权重加载失败。
    PointerHeadLoadFailed {
        /// 中文失败原因。
        detail: String,
    },
    /// llama worker 启动失败。
    WorkerStart {
        /// 中文失败原因。
        detail: String,
    },
}

impl std::fmt::Display for EmbeddedScoringWiringError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmbeddedUnavailable => write!(
                formatter,
                "System One embedded 评分未随本构建编译（feature `embedded` 未启用），评分端口不可用"
            ),
            Self::ManifestInvalid { detail } => {
                write!(formatter, "System One 发行 manifest 契约非法：{detail}")
            }
            Self::ModelMissing => write!(
                formatter,
                "System One 模型未安装，评分功能不可用；执行 `aemeath systemone download` 安装"
            ),
            Self::InvalidAssets { kind, detail } => {
                write!(formatter, "System One 模型资产校验失败：{kind}：{detail}")
            }
            Self::AssetContract { detail } => {
                write!(formatter, "System One 模型资产契约非法：{detail}")
            }
            Self::TokenizerLoadFailed { detail } => {
                write!(formatter, "评分 tokenizer 初始化失败：{detail}")
            }
            Self::PointerHeadLoadFailed { detail } => {
                write!(formatter, "评分 PointerHead 初始化失败：{detail}")
            }
            Self::WorkerStart { detail } => write!(formatter, "{detail}"),
        }
    }
}

impl std::error::Error for EmbeddedScoringWiringError {}

/// 生产 embedded 评分装配链（async；composition 经场景开关门内调用）。
///
/// - `models_dir`：模型安装根（生产为 `share::config::paths::systemone_models_dir()`）。
/// - `scoring_dir`：校准 artifact 与审计事件落盘目录。
/// - `manifest`：composition 注入的固定发行 manifest（契约先校验，零 IO 拒绝）。
///
/// 返回装配完成的 [`ScoringPort`]（Calibrated → Audited 外壳），或 typed 错误。
pub async fn wire_embedded_scoring(
    models_dir: PathBuf,
    scoring_dir: PathBuf,
    manifest: ModelManifest,
) -> Result<Arc<dyn ScoringPort>, EmbeddedScoringWiringError> {
    manifest
        .validate()
        .map_err(|error| EmbeddedScoringWiringError::ManifestInvalid {
            detail: error.to_string(),
        })?;
    #[cfg(not(feature = "embedded"))]
    {
        // feature 关闭：零 IO fail-closed（不解析资产状态、不启动 worker）。
        let _ = (models_dir, scoring_dir);
        Err(EmbeddedScoringWiringError::EmbeddedUnavailable)
    }
    #[cfg(feature = "embedded")]
    {
        let store = LocalModelAssetStore::new(models_dir, manifest.clone()).map_err(|error| {
            EmbeddedScoringWiringError::ManifestInvalid {
                detail: error.to_string(),
            }
        })?;
        // 异步边界：资产三态解析（adapter 内 spawn_blocking）与 worker 启动均 await。
        let state = store.installed_assets().await;
        let embedded = EmbeddedScoringAdapter::start(&state)
            .await
            .map_err(EmbeddedScoringWiringError::from)?;
        Ok(wrap_calibrated_audited(
            Arc::new(embedded),
            &manifest,
            &scoring_dir,
        ))
    }
}

/// 校准 + 审计外壳（装配链后两层）：审计 revision MUST 取自
/// `manifest.engine_revision`，审计事件落在 `scoring_dir/audit.jsonl`。
#[cfg(any(feature = "embedded", test))]
pub(crate) fn wrap_calibrated_audited(
    inner: Arc<dyn ScoringPort>,
    manifest: &ModelManifest,
    scoring_dir: &Path,
) -> Arc<dyn ScoringPort> {
    let store = CalibrationStore::new(scoring_dir.to_path_buf());
    let calibrated = Arc::new(CalibratedScoringAdapter::new(inner, &store));
    Arc::new(AuditedScoringAdapter::new(
        calibrated,
        manifest.engine_revision.clone(),
        scoring_dir.join(crate::constants::AUDIT_FILE),
        Arc::new(|| chrono::Utc::now().to_rfc3339()),
    ))
}

/// `EmbeddedInitError` → typed wiring 错误（分类不丢失，消息保持中文）。
#[cfg(feature = "embedded")]
impl From<crate::adapters::embedded::EmbeddedInitError> for EmbeddedScoringWiringError {
    fn from(error: crate::adapters::embedded::EmbeddedInitError) -> Self {
        use crate::adapters::embedded::EmbeddedInitError as InitError;
        match error {
            InitError::ModelMissing => Self::ModelMissing,
            InitError::InvalidAssets { kind, detail } => Self::InvalidAssets { kind, detail },
            InitError::AssetContract { detail } => Self::AssetContract { detail },
            InitError::TokenizerLoadFailed { detail } => Self::TokenizerLoadFailed { detail },
            InitError::PointerHeadLoadFailed { detail } => Self::PointerHeadLoadFailed { detail },
            InitError::WorkerStart { source } => Self::WorkerStart {
                detail: source.to_string(),
            },
        }
    }
}

#[cfg(test)]
#[path = "wiring_tests.rs"]
mod tests;
