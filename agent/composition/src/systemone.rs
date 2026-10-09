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

/// System One 评分端口的场景分配：四槽与四个场景开关一一对应（开关关闭或
/// 装配失败为 `None`，消费点回退原路径）。槽位 NEVER 复用跨场景语义——
/// 消费端按自身场景取槽，杜绝「A 开关点亮 B 场景槽位」的隐性耦合。
#[derive(Clone, Default)]
pub struct ScoringPortAssignment {
    /// memory rerank 场景槽位（memory search 词法召回 top-N Choice 重排）。
    pub for_memory_rerank: Option<Arc<dyn systemone::ScoringPort>>,
    /// memory recall 场景槽位（per-message 记忆主动召回 reminder）。
    pub for_memory_recall: Option<Arc<dyn systemone::ScoringPort>>,
    /// skill match 场景槽位（ToolSearch 语义重排，#1835 口径拍板）。
    pub for_skill_match: Option<Arc<dyn systemone::ScoringPort>>,
    /// policy triage 场景槽位（权限/风险预筛，单向加严，消费端随 #1836 接入）。
    pub for_policy_triage: Option<Arc<dyn systemone::ScoringPort>>,
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

/// `aemeath systemone download` 的命令结果（CLI 只渲染，不决策）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemoneDownloadReport {
    /// 本地已有有效安装（幂等命中，未发生覆盖）。
    pub already_installed: bool,
    /// 已安装的 engine revision。
    pub revision: String,
    /// 已安装 revision 的根目录。
    pub install_root: std::path::PathBuf,
}

impl SystemoneDownloadReport {
    /// CLI 退出码：成功 / 幂等 → 0。
    pub fn exit_code(&self) -> i32 {
        0
    }
}

/// `aemeath systemone download` 的退出结局：成功或 typed 失败（含退出码与
/// 中文消息），CLI NEVER 字符串匹配决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemoneDownloadExit {
    /// 下载安装成功或幂等命中已有安装。
    Success(SystemoneDownloadReport),
    /// typed 失败（manifest 未提供 / 契约非法 / 安装状态无效 / 下载或安装失败）。
    Failure {
        /// 中文失败消息。
        message: String,
        /// 非零退出码。
        exit_code: i32,
    },
}

/// `aemeath systemone download` 生产入口：manifest 取自生产发行 manifest 源，
/// 模型目录取 `~/.agents/models/systemone`，重定向 host 白名单取 HF CDN 域。
pub async fn run_systemone_download(user_agent: &str) -> SystemoneDownloadExit {
    let manifest = ProductionReleaseManifest.release_manifest();
    run_systemone_download_inner(
        manifest,
        share::config::paths::systemone_models_dir(),
        user_agent,
        &crate::constants::HF_CDN_REDIRECT_HOSTS,
    )
    .await
}

/// 可注入装配入口（wiring 测试用）：manifest 缺失 → typed 失败 fail-closed，
/// 不构造下载链、不触碰模型目录；manifest 有效则经 systemone 工厂装配真实
/// 生产链（本地存储 + https 抓取器）执行下载用例。
pub async fn run_systemone_download_with(
    manifest: Option<systemone::ModelManifest>,
    models_dir: std::path::PathBuf,
    user_agent: &str,
) -> SystemoneDownloadExit {
    run_systemone_download_inner(manifest, models_dir, user_agent, &[]).await
}

async fn run_systemone_download_inner(
    manifest: Option<systemone::ModelManifest>,
    models_dir: std::path::PathBuf,
    user_agent: &str,
    allowed_redirect_hosts: &[&str],
) -> SystemoneDownloadExit {
    let Some(manifest) = manifest else {
        return SystemoneDownloadExit::Failure {
            message: "System One 发行模型 manifest 未提供：当前构建未内置经确认的模型\
                      下载元数据（URL / SHA-256），请等待后续发行版本。"
                .to_owned(),
            exit_code: 1,
        };
    };
    let service = match systemone::wire_model_download_service(
        models_dir,
        user_agent,
        manifest,
        allowed_redirect_hosts,
    ) {
        Ok(service) => service,
        Err(error) => {
            return SystemoneDownloadExit::Failure {
                message: error.to_string(),
                exit_code: error.exit_code(),
            };
        }
    };
    match service.download().await {
        Ok(outcome) => SystemoneDownloadExit::Success(SystemoneDownloadReport {
            already_installed: matches!(
                outcome,
                systemone::DownloadOutcome::AlreadyInstalled { .. }
            ),
            revision: outcome.revision().to_owned(),
            install_root: outcome.install_root().to_path_buf(),
        }),
        Err(error) => SystemoneDownloadExit::Failure {
            message: error.to_string(),
            exit_code: error.exit_code(),
        },
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

    /// 装配 raw 引擎链（资产解析 → llama worker；不含校准 / 审计外壳）。
    async fn wire(
        &self,
        manifest: &systemone::ModelManifest,
    ) -> Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError>;

    /// 按场景包装校准 + 审计外壳（生产：`wrap_calibrated_audited`，事件带
    /// 场景标签归因；测试默认透传 raw，避免测试向真实 scoring 目录落盘）。
    fn wrap(
        &self,
        raw: Arc<dyn systemone::ScoringPort>,
        manifest: &systemone::ModelManifest,
        scenario: &'static str,
    ) -> Arc<dyn systemone::ScoringPort> {
        raw
    }
}

/// 生产发行 manifest 源：当前发行（kev 0.8B 合并 Q8_0，托管于 Hugging Face
/// 公开仓库 `rushsinging/aemeath-systemone-kev`）。
///
/// URL / 长度 / SHA-256 是下载链的**信任根**：资产内容任何变化（重新转换、
/// 重新量化）MUST 重新上传并同步更新 constants 的发行常量与 revision。
pub(crate) struct ProductionReleaseManifest;

impl ReleaseManifestSource for ProductionReleaseManifest {
    fn release_manifest(&self) -> Option<systemone::ModelManifest> {
        Some(systemone::ModelManifest {
            schema_version: 1,
            engine_revision: crate::constants::SYSTEMONE_RELEASE_REVISION.to_owned(),
            hidden_size: 1024,
            pointer_dimension: 256,
            temperature: 2.351_095_8,
            supported_platforms: vec![systemone::required_platform().to_owned()],
            assets: crate::constants::SYSTEMONE_RELEASE_ASSETS
                .iter()
                .map(|(path, byte_length, sha256)| systemone::ModelAsset {
                    url: format!("{}/{}", crate::constants::SYSTEMONE_RELEASE_BASE_URL, path),
                    byte_length: *byte_length,
                    sha256: (*sha256).to_owned(),
                    path: (*path).to_owned(),
                })
                .collect(),
        })
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
        manifest: &systemone::ModelManifest,
    ) -> Result<Arc<dyn systemone::ScoringPort>, systemone::EmbeddedScoringWiringError> {
        systemone::wire_embedded_scoring_raw(
            share::config::paths::systemone_models_dir(),
            manifest.clone(),
        )
        .await
    }

    fn wrap(
        &self,
        raw: Arc<dyn systemone::ScoringPort>,
        manifest: &systemone::ModelManifest,
        scenario: &'static str,
    ) -> Arc<dyn systemone::ScoringPort> {
        systemone::wrap_calibrated_audited(raw, manifest, &scoring_dir(), scenario)
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
        &ProductionReleaseManifest,
    )
    .await
}

/// 装配矩阵（可注入 fake，供契约测试覆盖全关 / feature 关闭 / 资产失败 / 成功链）。
pub(crate) async fn assemble_scoring_ports_with(
    scoring: &share::config::ScoringConfig,
    factory: &dyn EmbeddedScoringFactory,
    source: &dyn ReleaseManifestSource,
) -> ScoringAssembly {
    let any_scenario_enabled = scoring.enabled
        && (scoring.memory_rerank
            || scoring.memory_recall
            || scoring.skill_match
            || scoring.policy_triage);
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
    match factory.wire(&manifest).await {
        Ok(raw_port) => ScoringAssembly {
            assignment: ScoringPortAssignment {
                for_memory_rerank: scoring
                    .memory_rerank
                    .then(|| factory.wrap(raw_port.clone(), &manifest, "memory_rerank")),
                for_memory_recall: scoring
                    .memory_recall
                    .then(|| factory.wrap(raw_port.clone(), &manifest, "memory_recall")),
                for_skill_match: scoring
                    .skill_match
                    .then(|| factory.wrap(raw_port.clone(), &manifest, "skill_match")),
                for_policy_triage: scoring
                    .policy_triage
                    .then(|| factory.wrap(raw_port.clone(), &manifest, "policy_triage")),
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
