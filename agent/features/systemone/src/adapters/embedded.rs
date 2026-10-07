//! EmbeddedScoringAdapter：生产 `ScoringPort` 的 embedded llama.cpp facade。
//!
//! 职责边界（设计 §4.1）：
//! - 初始化 fail-closed：资产缺失 / tokenizer / PointerHead / worker 任一失败
//!   返回 [`EmbeddedInitError`]，**NEVER 构造 port**、NEVER 下载、NEVER 回退 HTTP；
//!   启动提醒由后续 composition 消费该 typed 错误。
//! - 单次推理：published language → kev causal row（owned token ids）→ 有界
//!   channel 提交专用 worker 线程 → 收回 owned hidden vectors → domain
//!   [`PointerHead`] → `ScoringAnswer`；任一环节失败映射 `ScoringUnavailable`。
//! - 校准级别：PointerHead 已在 logit 内除以出厂温度，答案标记
//!   [`CalibrationLevel::Temperature`]；在线温度 artifact 仍由外层
//!   `CalibratedScoringAdapter` 决定是否叠加。

use async_trait::async_trait;

use crate::adapters::kev_answer_mapping::build_kev_answer;
use crate::adapters::kev_causal_row::KevCausalRowBuilder;
use crate::adapters::kev_hf_tokenizer::HfKevTokenizer;
use crate::adapters::kev_question_plan::{plan_kev_question, KevQuestionPlan};
use crate::adapters::llama_worker::{
    start_llama_worker, EmbeddedWorkerClient, LlamaWorkerConfig, WorkerInitError,
};
use crate::adapters::pointer_head_loader::load_pointer_head;
use crate::constants::{
    EMBEDDED_CONTEXT_TOKENS, EMBEDDED_UBATCH_TOKENS, TOKENIZER_JSON_RELATIVE_PATH,
};
use crate::domain::{
    PointerHead, ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
use crate::ports::{InvalidAssetKind, ModelAssetState, ScoringPort};

/// embedded 评分初始化失败分类（启动期禁用 System One 的唯一返回通道）。
#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddedInitError {
    /// 本地模型资产未安装（提示执行 `aemeath systemone download`）。
    ModelMissing,
    /// 已安装但校验失败（损坏 / 不可读 / 契约不支持）。
    InvalidAssets {
        /// 失败类别。
        kind: InvalidAssetKind,
        /// 中文失败原因。
        detail: String,
    },
    /// manifest 结构不符合 embedded 装配契约。
    AssetContract { detail: String },
    /// tokenizer 资产加载失败。
    TokenizerLoadFailed { detail: String },
    /// PointerHead 权重加载失败。
    PointerHeadLoadFailed { detail: String },
    /// worker 启动失败（平台不支持、模型加载失败、上下文创建失败等）。
    WorkerStart { source: WorkerInitError },
}

impl std::fmt::Display for EmbeddedInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelMissing => write!(
                formatter,
                "System One 模型未安装，评分功能不可用；执行 `aemeath systemone download` 安装"
            ),
            Self::InvalidAssets { detail, .. } => {
                write!(formatter, "System One 模型资产校验失败：{detail}")
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
            Self::WorkerStart { source } => write!(formatter, "{source}"),
        }
    }
}

impl std::error::Error for EmbeddedInitError {}

impl std::fmt::Debug for EmbeddedScoringAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmbeddedScoringAdapter")
            .field("hidden_size", &self.hidden_size)
            .finish_non_exhaustive()
    }
}

/// embedded 评分端口：kev causal row 编码 + 专用 llama worker + domain PointerHead。
pub struct EmbeddedScoringAdapter {
    /// 有界 channel 对端（worker 串行服务，逐请求 oneshot 配对）。
    client: EmbeddedWorkerClient,
    /// published language → causal row（持有生产 tokenizer）。
    row_builder: KevCausalRowBuilder,
    /// 外置指针头（hidden → 概率，纯 domain 数学）。
    pointer_head: PointerHead,
    /// manifest 声明的 hidden 宽度（回传向量逐次对账）。
    hidden_size: usize,
}

impl EmbeddedScoringAdapter {
    /// 生产装配入口：解析已校验资产 → 加载 tokenizer / PointerHead → 启动 worker。
    ///
    /// 任一环节失败返回 [`EmbeddedInitError`]，NEVER 构造 port。
    pub async fn start(assets: &ModelAssetState) -> Result<Self, EmbeddedInitError> {
        let installed = match assets {
            ModelAssetState::Installed(installed) => installed,
            ModelAssetState::Missing => return Err(EmbeddedInitError::ModelMissing),
            ModelAssetState::Invalid { kind, detail } => {
                return Err(EmbeddedInitError::InvalidAssets {
                    kind: *kind,
                    detail: detail.clone(),
                });
            }
        };
        let manifest = installed.manifest();
        let model_asset = manifest
            .model_asset()
            .map_err(|error| embedded_asset_contract(error))?;
        let pointer_head_asset = manifest
            .pointer_head_asset()
            .map_err(|error| embedded_asset_contract(error))?;
        let install_root = installed.install_root();
        Self::start_from_paths(
            install_root.join(&model_asset.path),
            install_root.join(TOKENIZER_JSON_RELATIVE_PATH),
            install_root.join(&pointer_head_asset.path),
            manifest.hidden_size as usize,
            manifest.pointer_dimension as usize,
            manifest.temperature,
        )
        .await
    }

    /// 按显式路径装配（`start` 的路径解析后置步骤，亦供真实模型验证测试复用）。
    pub(crate) async fn start_from_paths(
        model_path: std::path::PathBuf,
        tokenizer_path: std::path::PathBuf,
        pointer_head_path: std::path::PathBuf,
        hidden_size: usize,
        pointer_dimension: usize,
        temperature: f32,
    ) -> Result<Self, EmbeddedInitError> {
        let tokenizer = HfKevTokenizer::from_file(&tokenizer_path).map_err(|error| {
            EmbeddedInitError::TokenizerLoadFailed {
                detail: error.to_string(),
            }
        })?;
        let row_builder = KevCausalRowBuilder::new(tokenizer);
        let pointer_head = load_pointer_head(
            &pointer_head_path,
            hidden_size,
            pointer_dimension,
            temperature,
        )
        .map_err(|error| EmbeddedInitError::PointerHeadLoadFailed {
            detail: error.to_string(),
        })?;
        let client = start_llama_worker(LlamaWorkerConfig {
            model_path,
            hidden_size,
            context_tokens: EMBEDDED_CONTEXT_TOKENS,
            ubatch_tokens: EMBEDDED_UBATCH_TOKENS,
        })
        .await
        .map_err(|source| EmbeddedInitError::WorkerStart { source })?;
        Ok(Self::from_parts(
            client,
            row_builder,
            pointer_head,
            hidden_size,
        ))
    }

    /// 装配已构造的部件（生产 `start` 的收尾步骤；测试经 fake worker / fake tokenizer 注入）。
    pub(crate) fn from_parts(
        client: EmbeddedWorkerClient,
        row_builder: KevCausalRowBuilder,
        pointer_head: PointerHead,
        hidden_size: usize,
    ) -> Self {
        Self {
            client,
            row_builder,
            pointer_head,
            hidden_size,
        }
    }
}

#[async_trait]
impl ScoringPort for EmbeddedScoringAdapter {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        if questions.is_empty() {
            return Ok(Vec::new());
        }
        let plans: Vec<KevQuestionPlan> = questions.iter().map(plan_kev_question).collect();
        let rows = self
            .row_builder
            .build_rows(state.as_str(), &plans)
            .map_err(|error| ScoringUnavailable::new(UnavailableKind::Schema, error.to_string()))?;
        let row_vectors = self.client.run_rows(rows).await?;
        if row_vectors.len() != plans.len() {
            return Err(ScoringUnavailable::new(
                UnavailableKind::Server,
                format!(
                    "worker 返回的 hidden 数量与题目数不符：期望 {}，实际 {}",
                    plans.len(),
                    row_vectors.len()
                ),
            ));
        }
        let mut answers = Vec::with_capacity(plans.len());
        for (plan, vectors) in plans.iter().zip(row_vectors) {
            if vectors.decide.len() != self.hidden_size {
                return Err(ScoringUnavailable::new(
                    UnavailableKind::Server,
                    format!(
                        "decide hidden 维度不符：期望 {}，实际 {}",
                        self.hidden_size,
                        vectors.decide.len()
                    ),
                ));
            }
            let mut hidden_options = Vec::with_capacity(vectors.options.len() * self.hidden_size);
            for (index, option) in vectors.options.iter().enumerate() {
                if option.len() != self.hidden_size {
                    return Err(ScoringUnavailable::new(
                        UnavailableKind::Server,
                        format!(
                            "第 {index} 个选项的 hidden 维度不符：期望 {}，实际 {}",
                            self.hidden_size,
                            option.len()
                        ),
                    ));
                }
                hidden_options.extend_from_slice(option);
            }
            let probabilities = self
                .pointer_head
                .score_options(&vectors.decide, &hidden_options)
                .map_err(|error| {
                    ScoringUnavailable::new(
                        UnavailableKind::Server,
                        format!("PointerHead 评分失败：{error}"),
                    )
                })?;
            let probability_values: Vec<f64> = probabilities
                .iter()
                .map(|probability| f64::from(*probability))
                .collect();
            let answer = build_kev_answer(plan, &probability_values).map_err(|error| {
                ScoringUnavailable::new(UnavailableKind::Server, error.to_string())
            })?;
            answers.push(answer);
        }
        Ok(answers)
    }
}

/// manifest 资产角色解析失败 → 装配契约错误（中文明细复用 domain Display）。
fn embedded_asset_contract(error: crate::domain::ModelManifestError) -> EmbeddedInitError {
    EmbeddedInitError::AssetContract {
        detail: error.to_string(),
    }
}

#[cfg(test)]
#[path = "embedded_tests.rs"]
mod tests;
