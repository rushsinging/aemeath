//! SystemOne：System One 决策模型评分服务（状态 × 候选集 → 概率分布）。
//!
//! # Published Language
//!
//! | 类 | 实体 | 消费者 |
//! |---|---|---|
//! | `wire_*` 工厂 | `wire_embedded_scoring`（生产 embedded 装配链：资产解析 → llama worker → 校准 → 审计）；`wire_http_scoring_port`（feature `http-adapter`，测试 / eval 对分专用） | 生产 composition / 测试 / eval |
//! | 适配器 | `EmbeddedScoringAdapter` / `EmbeddedInitError`（feature `embedded`，macOS arm64 llama.cpp worker） | 生产评分装配 |
//! | 数据 | `ScoringQuestion` / `ScoringAnswer` / `ScoringState` / `CalibrationLevel` | 消费场景（memory / skills / policy） |
//! | 数据 | `ModelManifest` / `PointerHead` / `PointerHeadWeights` | 模型下载、存储与 embedded 评分装配 |
//! | 端口 | `ScoringPort` / `CalibrationPort` / `ModelAssetPort` | 消费场景只依赖端口，NEVER 感知引擎型号 |
//! | 端口 | `ArtifactFetcherPort` / `ModelInstallerPort` | 手动下载用例只依赖端口，application NEVER 反向依赖 adapter |
//! | 服务 | `ModelDownloadService` / `DownloadOutcome` / `ModelDownloadError` | `aemeath systemone download` 编排（成功/幂等 → 0，失败 → 非零） |
//! | 错误 | `ScoringUnavailable` / `PointerHeadError` / `ModelManifestError` | 消费点据此静默回退原路径 |
//!
//! 设计依据：`docs/design/02-modules/systemone/01-systemone-scoring.md`。

mod constants;
pub(crate) use constants::LOG_TARGET;
mod state;

mod adapters;
mod application;
mod domain;
mod ports;
mod wiring;

pub use adapters::audited::AuditedScoringAdapter;
pub use adapters::calibrated::CalibratedScoringAdapter;
pub use adapters::calibration_store::{CalibrationArtifact, CalibrationStore};
#[cfg(feature = "embedded")]
pub use adapters::embedded::{EmbeddedInitError, EmbeddedScoringAdapter};
pub use adapters::event_jsonl::JsonlSegmentScoringEventStore;
pub use adapters::fetch_http::HttpArtifactFetcher;
#[cfg(feature = "http-adapter")]
pub use adapters::jev_http::JevHttpScoringAdapter;
#[cfg(feature = "embedded")]
pub use adapters::llama_worker::WorkerInitError;
pub use adapters::model_assets::{
    LocalModelAssetStore, ModelInstallError, PreparedStagedInstall, StagedInstallCommit,
    StagingDirectory,
};
pub use adapters::null::NullScoringAdapter;
pub use application::{
    DownloadOutcome, ModelDownloadError, ModelDownloadErrorKind, ModelDownloadService,
};

pub use constants::EMBEDDED_SCORING_AVAILABLE;
pub use domain::{
    required_platform, AnswerRejected, CalibrationLevel, CriterionSnapshot, ModelAsset,
    ModelManifest, ModelManifestError, NoulCriteria, PointerHead, PointerHeadError,
    PointerHeadWeights, QuestionRejected, ScoringAnswer, ScoringAnswerSnapshot, ScoringEvent,
    ScoringQuestion, ScoringQuestionSnapshot, ScoringRankingSnapshot, ScoringState,
    ScoringUnavailable, UnavailableKind,
};
pub use ports::{
    ArtifactFetchError, ArtifactFetchErrorKind, ArtifactFetcherPort, CalibrationObservation,
    CalibrationPort, InstalledAssets, InvalidAssetKind, ModelAssetPort, ModelAssetState,
    ModelInstallPortError, ModelInstallPortErrorKind, ModelInstallerPort, ModelStagingArea,
    ScoringPort, StagedInstallOutcome,
};
pub use wiring::{
    wire_embedded_scoring, wire_embedded_scoring_per_scenario, wire_embedded_scoring_raw,
    wire_model_download_service, wrap_calibrated_audited, EmbeddedScoringWiringError,
};

/// Jev HTTP 评分装配链：JevHttp → Calibrated（读温度 artifact）→ Audited（落评分事件）。
///
/// 设计 §4.3 HTTP adapter 退役边界：仅供测试 / eval 对分与回归基准使用，
/// 生产 composition **NEVER** 调用；默认构建不提供本工厂。
///
/// `event_retention_days`：评分事件保留天数（测试 / eval 侧传
/// `share::config::scoring::DEFAULT_EVENT_RETENTION_DAYS` 或显式配置值）。
#[cfg(feature = "http-adapter")]
pub fn wire_http_scoring_port(
    base_url: &str,
    model: &str,
    timeout: std::time::Duration,
    scoring_dir: std::path::PathBuf,
    event_retention_days: u32,
) -> std::sync::Arc<dyn ScoringPort> {
    let http = std::sync::Arc::new(JevHttpScoringAdapter::new(base_url, model, timeout));
    let store = CalibrationStore::new(scoring_dir.clone());
    let calibrated = std::sync::Arc::new(CalibratedScoringAdapter::new(http, &store));
    let event_store = JsonlSegmentScoringEventStore::new(scoring_dir, event_retention_days);
    std::sync::Arc::new(AuditedScoringAdapter::new(
        calibrated,
        model.to_owned(),
        event_store,
        std::sync::Arc::new(|| chrono::Utc::now().to_rfc3339()),
        // HTTP 评分仅供测试 / eval 对分（设计 §4.3），场景标签固定 eval。
        "eval_http",
    ))
}
