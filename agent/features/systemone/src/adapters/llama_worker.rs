//! embedded worker 协议与线程循环：llama.cpp 的 backend/model/context/batch
//! 全部驻留专用 worker 线程，Tokio facade 只经有界 channel 传递
//! **owned token ids 与 owned hidden vectors**，NEVER 跨线程搬运 C/C++ 上下文。
//!
//! - [`CausalRow`]：一题一行的 owned token ids + readout 偏移。
//! - [`RowHiddenVectors`]：worker 回传的 owned hidden 向量。
//! - [`EmbeddedWorkerClient`]：facade 侧句柄（只持 sender，可并发提交；
//!   worker 串行处理，每个请求经独立 oneshot 回执配对，取消安全）。
//! - native 初始化（`LlamaBackend` / `LlamaModel` / `LlamaContext`）在
//!   [`spawn_worker_thread`] 的线程闭包内完成，错误经初始化通道回传；
//!   初始化失败 NEVER 构造 client。

use std::path::PathBuf;

use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::domain::{ScoringUnavailable, UnavailableKind};

/// 一条 kev causal row：state 前缀 + 单题分支的 owned token ids，
/// 以及分支内 decide / 各选项的 readout 偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CausalRow {
    /// 完整 row 的 token ids（state 前缀 + 分支）。
    pub(crate) token_ids: Vec<i32>,
    /// decide 位置在 `token_ids` 内的偏移。
    pub(crate) decide_offset: usize,
    /// 各选项 option-end 位置在 `token_ids` 内的偏移（保选项顺序）。
    pub(crate) option_offsets: Vec<usize>,
}

impl CausalRow {
    /// 构造 row（生产构造方为 `KevCausalRowBuilder`）。
    pub(crate) fn new(
        token_ids: Vec<i32>,
        decide_offset: usize,
        option_offsets: Vec<usize>,
    ) -> Self {
        Self {
            token_ids,
            decide_offset,
            option_offsets,
        }
    }
}

/// 一行 row 的 owned hidden 向量：decide 位 + 各选项位，长度 = 模型 hidden size。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowHiddenVectors {
    /// decide 位置的 hidden 向量。
    pub(crate) decide: Vec<f32>,
    /// 各选项位置的 hidden 向量（保选项顺序）。
    pub(crate) options: Vec<Vec<f32>>,
}

impl RowHiddenVectors {
    /// 构造回传向量。
    pub(crate) fn new(decide: Vec<f32>, options: Vec<Vec<f32>>) -> Self {
        Self { decide, options }
    }
}

/// 单次推理失败原因（中文 detail 随 `ScoringUnavailable` 透出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerFailure {
    detail: String,
}

impl WorkerFailure {
    /// 构造失败原因（detail 必须为中文，来自 native 层错误转换）。
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "评分 worker 推理失败：{}", self.detail)
    }
}

/// worker 对单个请求的回执。
pub(crate) enum WorkerResponse {
    /// 逐行 owned hidden 向量，顺序与请求 rows 一致。
    Hidden(Vec<RowHiddenVectors>),
    /// 本次推理失败（worker 继续服务后续请求）。
    Failed(WorkerFailure),
}

/// worker 请求：owned rows + 该请求专属的回执通道。
pub(crate) struct WorkerRequest {
    /// 待前向的 owned rows。
    pub(crate) rows: Vec<CausalRow>,
    /// 请求方的回执通道（请求方取消时发送失败，worker 忽略并继续）。
    pub(crate) respond_to: oneshot::Sender<WorkerResponse>,
}

/// 前向引擎契约：native llama.cpp 与测试 fake 共用的 contract seam。
pub(crate) trait RowEmbeddingEngine {
    /// 对一批 owned row 前向，返回逐行 owned hidden 向量。
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure>;
}

/// worker 初始化失败分类（启动期 fail-closed：不构造 client，由 composition 转 startup notice）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerInitError {
    /// 首批平台之外，embedded 运行能力未实现（NEVER 回退 HTTP）。
    UnsupportedPlatform { platform: String },
    /// llama backend 初始化失败。
    BackendInitFailed { detail: String },
    /// GGUF 模型加载失败（含模型文件缺失）。
    ModelLoadFailed { detail: String },
    /// 上下文创建失败（embeddings / pooling 参数不被支持等）。
    ContextCreateFailed { detail: String },
    /// 模型输出维度与 manifest 声明不一致。
    HiddenSizeMismatch { expected: usize, found: usize },
    /// worker 线程在初始化回执前异常退出。
    WorkerThreadStopped,
}

impl std::fmt::Display for WorkerInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform { platform } => {
                write!(
                    formatter,
                    "当前平台 {platform} 不支持 embedded llama.cpp 评分"
                )
            }
            Self::BackendInitFailed { detail } => {
                write!(formatter, "llama backend 初始化失败：{detail}")
            }
            Self::ModelLoadFailed { detail } => {
                write!(formatter, "模型加载失败：{detail}")
            }
            Self::ContextCreateFailed { detail } => {
                write!(formatter, "评分上下文创建失败：{detail}")
            }
            Self::HiddenSizeMismatch { expected, found } => {
                write!(
                    formatter,
                    "模型 hidden 维度不符：期望 {expected}，实际 {found}"
                )
            }
            Self::WorkerThreadStopped => write!(formatter, "评分 worker 线程在初始化阶段异常退出"),
        }
    }
}

impl std::error::Error for WorkerInitError {}

/// facade 侧 worker 句柄：只持有有界请求通道的 sender。
#[derive(Debug)]
pub(crate) struct EmbeddedWorkerClient {
    request_tx: tokio_mpsc::Sender<WorkerRequest>,
}

impl EmbeddedWorkerClient {
    /// 提交 owned rows 并收回 owned hidden vectors。
    ///
    /// 错误映射（设计 §4.4 单次推理失败 → `ScoringUnavailable`）：
    /// - worker 已退出（提交或回执通道关闭）→ `Connect`；
    /// - worker 报告的推理失败 → `Server`（明细为 worker 的中文原因）。
    pub(crate) async fn run_rows(
        &self,
        rows: Vec<CausalRow>,
    ) -> Result<Vec<RowHiddenVectors>, ScoringUnavailable> {
        let (respond_to, response_rx) = oneshot::channel();
        self.request_tx
            .send(WorkerRequest { rows, respond_to })
            .await
            .map_err(|_| {
                ScoringUnavailable::new(
                    UnavailableKind::Connect,
                    "评分 worker 已退出，无法提交推理请求",
                )
            })?;
        let response = response_rx.await.map_err(|_| {
            ScoringUnavailable::new(UnavailableKind::Connect, "评分 worker 在返回结果前退出")
        })?;
        match response {
            WorkerResponse::Hidden(vectors) => Ok(vectors),
            WorkerResponse::Failed(failure) => Err(ScoringUnavailable::new(
                UnavailableKind::Server,
                failure.to_string(),
            )),
        }
    }
}

/// native worker 启动配置：模型路径、期望 hidden 宽度与上下文容量。
pub(crate) struct LlamaWorkerConfig {
    /// `model.gguf` 绝对路径。
    pub(crate) model_path: PathBuf,
    /// manifest 声明的 hidden size（init 期与模型对账）。
    pub(crate) hidden_size: usize,
    /// 上下文 / 批容量 token 数。
    pub(crate) context_tokens: u32,
    /// 单次计算 chunk（ubatch）容量 token 数。
    pub(crate) ubatch_tokens: u32,
}

/// 在专用线程内初始化引擎并进入服务循环。
///
/// native 类型只存在于线程闭包内部；调用方经返回的初始化通道 await
/// [`EmbeddedWorkerClient`]，初始化失败时 NEVER 产生 client。
pub(crate) fn spawn_worker_thread<E, F>(
    init_engine: F,
) -> oneshot::Receiver<Result<EmbeddedWorkerClient, WorkerInitError>>
where
    F: FnOnce() -> Result<E, WorkerInitError> + Send + 'static,
    E: RowEmbeddingEngine + 'static,
{
    let (request_tx, mut request_rx) = tokio_mpsc::channel::<WorkerRequest>(1);
    let (init_tx, init_rx) = oneshot::channel();
    std::thread::spawn(move || {
        let mut engine = match init_engine() {
            Ok(engine) => engine,
            Err(error) => {
                let _ = init_tx.send(Err(error));
                return;
            }
        };
        let _ = init_tx.send(Ok(EmbeddedWorkerClient { request_tx }));
        while let Some(request) = request_rx.blocking_recv() {
            let WorkerRequest { rows, respond_to } = request;
            let response = match engine.embed_rows(rows) {
                Ok(vectors) => WorkerResponse::Hidden(vectors),
                Err(failure) => WorkerResponse::Failed(failure),
            };
            // 请求方已取消时发送失败：忽略并继续服务下一个请求（单次失败不熔断）。
            let _ = respond_to.send(response);
        }
    });
    init_rx
}

/// 启动 native llama worker：macOS arm64 走真实初始化，其余平台报告不支持。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) async fn start_llama_worker(
    config: LlamaWorkerConfig,
) -> Result<EmbeddedWorkerClient, WorkerInitError> {
    let init_receiver = spawn_worker_thread(move || {
        crate::adapters::llama_native_engine::init_llama_row_engine(&config)
    });
    init_receiver
        .await
        .map_err(|_| WorkerInitError::WorkerThreadStopped)?
}

/// 启动 native llama worker：首批平台之外 fail-closed，NEVER 回退其他评分后端。
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(crate) async fn start_llama_worker(
    _config: LlamaWorkerConfig,
) -> Result<EmbeddedWorkerClient, WorkerInitError> {
    Err(WorkerInitError::UnsupportedPlatform {
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    })
}

#[cfg(test)]
#[path = "llama_worker_tests.rs"]
mod tests;
