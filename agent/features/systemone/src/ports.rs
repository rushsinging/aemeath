//! 评分、校准与模型资产端口：消费方只依赖端口，NEVER 直接感知引擎型号。

use std::path::PathBuf;

use async_trait::async_trait;

use crate::domain::{
    CalibrationLevel, ModelManifest, ModelManifestError, ScoringAnswer, ScoringQuestion,
    ScoringState, ScoringUnavailable,
};

/// 评分端口：批量评分；服务不可用返回 `ScoringUnavailable`（消费点据此回退）。
#[async_trait]
pub trait ScoringPort: Send + Sync {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable>;
}

/// 一条「评分 + 后续观测标签」记录，供离线/在线校准拟合。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalibrationObservation {
    pub question: ScoringQuestion,
    pub probabilities: Vec<f64>,
    pub label: String,
    pub engine_revision: String,
    pub timestamp: String,
}

/// 校准端口：observe 回路落盘 + 当前生效校准 artifact。
#[async_trait]
pub trait CalibrationPort: Send + Sync {
    /// 落盘一条观测记录，供校准拟合。
    async fn observe(&self, record: CalibrationObservation);

    /// 当前生效的校准级别；无 artifact 时为 `CalibrationLevel::Raw`。
    fn current(&self) -> CalibrationLevel;
}

/// 已通过全部校验的模型资产：manifest 单一来源 + 安装根目录。
///
/// 字段私有：外部 NEVER 可凭字段直接构造，只能经先校验 manifest 的构造器 `new` 装配。
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledAssets {
    /// 校验通过的 manifest（路径、URL、字节数、SHA-256 的唯一声明处）。
    manifest: ModelManifest,
    /// 已安装 revision 的根目录。
    install_root: PathBuf,
}

impl InstalledAssets {
    /// 装配入口：先全量校验 manifest（`validate()`，非法/空资产即拒绝），通过才装配。
    ///
    /// 本 crate 的存储 adapter 与测试经此装配；字段仍私有，实例不可自行拼装。
    pub fn new(manifest: ModelManifest, install_root: PathBuf) -> Result<Self, ModelManifestError> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            install_root,
        })
    }

    /// 校验通过的 manifest（只读）。
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// 已安装 revision 的根目录（只读）。
    pub fn install_root(&self) -> &std::path::Path {
        &self.install_root
    }
}

/// 已安装但校验失败的原因类别：损坏、不可读、契约不支持。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidAssetKind {
    /// 内容损坏（长度 / SHA-256 与 manifest 不符）。
    Corrupt,
    /// 文件系统不可读（权限、IO 错误）。
    Unreadable,
    /// 契约不支持（schema 版本、维度或布局不符）。
    Unsupported,
}

impl std::fmt::Display for InvalidAssetKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt => write!(formatter, "模型资产已损坏"),
            Self::Unreadable => write!(formatter, "模型资产不可读"),
            Self::Unsupported => write!(formatter, "模型资产不受支持"),
        }
    }
}

/// 模型资产三态：fail-closed —— 只有 `Installed` 可加载，缺失与损坏分开报告。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelAssetState {
    /// 资产齐备且逐项校验通过。
    Installed(InstalledAssets),
    /// 本地未安装（提示用户执行手动下载命令）。
    Missing,
    /// 已安装但校验失败：typed 原因类别 + 中文 detail。
    Invalid {
        /// 失败类别（损坏 / 不可读 / 不支持）。
        kind: InvalidAssetKind,
        /// 中文失败原因。
        detail: String,
    },
}

/// 模型资产端口：只暴露「安装状态解析」一个职责。
///
/// 文件系统遍历、长度/SHA-256 计算与原子安装细节 MUST 留在 adapter 内；
/// 端口 NEVER 暴露下载、IO 或哈希操作。
#[async_trait]
pub trait ModelAssetPort: Send + Sync {
    /// 解析本地模型资产安装状态：齐备 → `Installed`，未安装 → `Missing`，
    /// 校验失败 → `Invalid`（永不把半成品识别为已安装）。
    async fn installed_assets(&self) -> ModelAssetState;
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;
