//! System One 评分端口装配：场景开关 → embedded 装配链 → typed 启动结果。
//!
//! 契约（设计 `docs/design/02-modules/systemone/01-systemone-scoring.md` §4.1/§4.3）：
//! - **场景开关全关：零成本**——不读取模型目录、不解析发行 manifest、不启动
//!   worker，返回 [`ScoringStartupOutcome::Disabled`]。
//! - **开关开启**：经注入的发行 manifest 源与 embedded 工厂装配；资产缺失 /
//!   损坏 / 不支持 / 初始化失败一律两个槽位 `None` + typed
//!   [`ScoringStartupOutcome`]，主聊天 **NEVER** 被阻断。
//! - **feature `systemone-embedded` 关闭**（默认构建）：typed
//!   [`ScoringStartupOutcome::EmbeddedUnavailable`]，零 IO。
//! - **NEVER 回退 HTTP 评分**；生产 manifest 源在仓库没有经确认的发行
//!   artifact 元数据（URL / SHA-256）时返回 `None` → typed
//!   [`ScoringStartupOutcome::ManifestUnavailable`]，**NEVER** 内置占位假数据。
//!
//! 本模块只产出 typed outcome；CLI / TUI 的启动提醒渲染由后续任务经 bootstrap
//! 透传承担，本任务不做任何 UI 逻辑。

use std::fmt::Display;
use std::sync::Arc;

use async_trait::async_trait;

/// System One 评分端口的场景分配：按场景开关分发给 memory rerank 与 memory
/// recall 消费点（skill match / policy triage 场景端口随各自场景任务补充）。
#[derive(Clone, Default)]
pub struct ScoringPortAssignment {
    /// memory rerank 场景槽位（开关关闭或装配失败为 `None`，消费点回退原路径）。
    pub for_memory_rerank: Option<Arc<dyn systemone::ScoringPort>>,
    /// memory recall 场景槽位（开关关闭或装配失败为 `None`，消费点回退原路径）。
    pub for_memory_recall: Option<Arc<dyn systemone::ScoringPort>>,
}

/// 启动期评分装配结果：variant 即 typed 分类，`detail` 为中文可读原因。
///
/// 后续任务据此透传 bootstrap startup notice；本任务生产 runtime 只消费日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoringStartupOutcome {
    /// 场景开关全关：零成本，未读取任何模型资产。
    Disabled,
    /// embedded 端口装配成功（至少一个场景开关开启）。
    Ready,
    /// 构建未启用 `systemone-embedded`：不加载模型、不回退 HTTP。
    EmbeddedUnavailable,
    /// 发行 manifest 未提供（仓库无经确认的发行 URL / SHA-256）：不加载模型、
    /// 不回退 HTTP。
    ManifestUnavailable,
    /// 模型未安装（detail 提示执行 `aemeath systemone download`）。
    ModelMissing {
        /// 中文原因（含手动下载命令提示）。
        detail: String,
    },
    /// 已安装但资产校验失败（损坏 / 不可读 / 不支持）。
    InvalidAssets {
        /// 中文原因（含失败类别）。
        detail: String,
    },
    /// 初始化失败（manifest 契约 / tokenizer / PointerHead / worker）。
    InitFailed {
        /// 中文原因。
        detail: String,
    },
}

impl Display for ScoringStartupOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(formatter, "System One 评分场景开关全关，未装配评分端口"),
            Self::Ready => write!(formatter, "System One embedded 评分端口已装配"),
            Self::EmbeddedUnavailable => write!(
                formatter,
                "System One embedded 评分未随本构建编译（feature `systemone-embedded` 未启用），\
                 评分端口为空（不加载模型、不回退 HTTP）"
            ),
            Self::ManifestUnavailable => write!(
                formatter,
                "System One 发行模型 manifest 未提供，评分端口为空（不加载模型、不回退 HTTP）"
            ),
            Self::ModelMissing { detail }
            | Self::InvalidAssets { detail }
            | Self::InitFailed { detail } => write!(formatter, "{detail}"),
        }
    }
}

impl From<systemone::EmbeddedScoringWiringError> for ScoringStartupOutcome {
    fn from(error: systemone::EmbeddedScoringWiringError) -> Self {
        match error {
            systemone::EmbeddedScoringWiringError::EmbeddedUnavailable => Self::EmbeddedUnavailable,
            systemone::EmbeddedScoringWiringError::ManifestInvalid { detail } => {
                Self::InitFailed { detail }
            }
            systemone::EmbeddedScoringWiringError::ModelMissing => Self::ModelMissing {
                detail: error.to_string(),
            },
            systemone::EmbeddedScoringWiringError::InvalidAssets { .. } => Self::InvalidAssets {
                detail: error.to_string(),
            },
            systemone::EmbeddedScoringWiringError::AssetContract { .. }
            | systemone::EmbeddedScoringWiringError::TokenizerLoadFailed { .. }
            | systemone::EmbeddedScoringWiringError::PointerHeadLoadFailed { .. }
            | systemone::EmbeddedScoringWiringError::WorkerStart { .. } => Self::InitFailed {
                detail: error.to_string(),
            },
        }
    }
}

/// 启动期一次性提醒（published value）：composition 装配时生成，经
/// `AgentClientBootstrap` 透传，CLI / TUI 只负责渲染，不读取配置、环境变量
/// 或模型目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupNotice {
    /// 中文提醒文本（含用户可执行的动作提示，如手动下载命令）。
    pub message: String,
}

/// typed 启动结果 → 启动提醒：`Disabled`（场景全关，用户未表达使用意图）
/// 与 `Ready`（正常装配）不打扰用户；其余 outcome 说明「场景开关开启但评分
/// 未生效」，各生成恰好一条中文提醒。
#[must_use]
pub(crate) fn scoring_startup_notices(outcome: &ScoringStartupOutcome) -> Vec<StartupNotice> {
    match outcome {
        ScoringStartupOutcome::Disabled | ScoringStartupOutcome::Ready => Vec::new(),
        outcome => vec![StartupNotice {
            message: outcome.to_string(),
        }],
    }
}

/// 装配结果：场景槽位分配 + typed 启动结果（两者一致性由装配矩阵契约测试锁定）。
#[must_use]
pub struct ScoringAssembly {
    /// 按场景开关分配的评分端口槽位。
    pub assignment: ScoringPortAssignment,
    /// typed 启动结果（生产 runtime 只消费此 outcome）。
    pub outcome: ScoringStartupOutcome,
}

/// 发行 manifest 注入源：唯一决定「当前构建期望哪份模型资产」的地方。
pub(crate) trait ReleaseManifestSource: Send + Sync {
    /// 当前发行 manifest；仓库尚无经确认的发行 artifact 元数据时为 `None`。
    fn release_manifest(&self) -> Option<systemone::ModelManifest>;
}

/// embedded 装配注入点：生产委托 `systemone::wire_embedded_scoring`，
/// 测试注入 fake 以覆盖装配矩阵（不读模型目录、不加载真实模型）。
#[async_trait]
pub(crate) trait EmbeddedScoringFactory: Send + Sync {
    /// 本构建是否具备 embedded 装配能力（systemone feature `embedded`）。
    fn available(&self) -> bool;

    /// 装配完整链（资产解析 → llama worker → 校准 → 审计）。
    async fn wire(
        &self,
        manifest: systemone::ModelManifest,
    ) -> Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError>;
}

/// 生产发行 manifest 源：**当前不提供**发行 artifact 元数据。
///
/// 仓库现状（docs / eval / git 全量检索）没有经用户确认的发行模型 URL 与
/// SHA-256；内置占位假数据会让真实下载 / 校验失败，故一律返回 `None` →
/// typed [`ScoringStartupOutcome::ManifestUnavailable`]。发行 manifest 落地后
/// 在此返回 `Some`（fixture 仅供测试）。
pub(crate) struct UnavailableReleaseManifest;

impl ReleaseManifestSource for UnavailableReleaseManifest {
    fn release_manifest(&self) -> Option<systemone::ModelManifest> {
        None
    }
}

/// 生产 embedded 工厂：委托 systemone 的唯一生产装配入口。
pub(crate) struct ProductionEmbeddedFactory;

#[async_trait]
impl EmbeddedScoringFactory for ProductionEmbeddedFactory {
    fn available(&self) -> bool {
        systemone::EMBEDDED_SCORING_AVAILABLE
    }

    async fn wire(
        &self,
        manifest: systemone::ModelManifest,
    ) -> Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError> {
        systemone::wire_embedded_scoring(
            share::config::paths::systemone_models_dir(),
            scoring_dir(),
            manifest,
        )
        .await
    }
}

/// `~/.agents/scoring/`：校准 artifact 与评分审计的落盘目录（设计 §观察回路）。
fn scoring_dir() -> std::path::PathBuf {
    share::config::paths::global_agents_dir().join("scoring")
}

/// 生产装配入口：场景全关零成本；开启时经生产工厂与生产 manifest 源装配。
pub async fn assemble_scoring_ports(scoring: &share::config::ScoringConfig) -> ScoringAssembly {
    assemble_scoring_ports_with(
        scoring,
        &ProductionEmbeddedFactory,
        &UnavailableReleaseManifest,
    )
    .await
}

/// 装配矩阵（可注入 fake，供契约测试覆盖全关 / feature 关闭 / 资产失败 / 成功链）。
pub(crate) async fn assemble_scoring_ports_with(
    scoring: &share::config::ScoringConfig,
    factory: &dyn EmbeddedScoringFactory,
    source: &dyn ReleaseManifestSource,
) -> ScoringAssembly {
    let any_scenario_enabled = scoring.memory_rerank
        || scoring.memory_recall
        || scoring.skill_match
        || scoring.policy_triage;
    if !any_scenario_enabled {
        // 零成本：不咨询 manifest 源、不读取模型目录、不解析 manifest、不启动 worker。
        return ScoringAssembly {
            assignment: ScoringPortAssignment::default(),
            outcome: ScoringStartupOutcome::Disabled,
        };
    }
    if !factory.available() {
        return ScoringAssembly {
            assignment: ScoringPortAssignment::default(),
            outcome: ScoringStartupOutcome::EmbeddedUnavailable,
        };
    }
    let Some(manifest) = source.release_manifest() else {
        return ScoringAssembly {
            assignment: ScoringPortAssignment::default(),
            outcome: ScoringStartupOutcome::ManifestUnavailable,
        };
    };
    match factory.wire(manifest).await {
        Ok(port) => ScoringAssembly {
            assignment: ScoringPortAssignment {
                for_memory_rerank: scoring.memory_rerank.then(|| port.clone()),
                for_memory_recall: scoring.memory_recall.then(|| port.clone()),
            },
            outcome: ScoringStartupOutcome::Ready,
        },
        Err(error) => ScoringAssembly {
            assignment: ScoringPortAssignment::default(),
            outcome: ScoringStartupOutcome::from(error),
        },
    }
}

#[cfg(test)]
#[path = "systemone_tests.rs"]
mod tests;
