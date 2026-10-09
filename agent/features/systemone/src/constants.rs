pub(crate) const LOG_TARGET: &str = "aemeath:agent:systemone";

/// Jev 评分端点路径（拼在配置的 base URL 后；仅 HTTP adapter 编译期存在）。
#[cfg(any(test, feature = "http-adapter"))]
pub(crate) const SYSTEMONE_PATH: &str = "/v1/systemone";

/// 连接预检窗口：规避 hyper-util 对 reusable body 的 connect 重试循环
///（实测 connect refused 等满总超时才失败）。仅 HTTP adapter 编译期存在。
#[cfg(any(test, feature = "http-adapter"))]
pub(crate) const PREFLIGHT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// 校准观测记录文件名（{scoring_dir}/observations.jsonl）。
pub(crate) const OBSERVATIONS_FILE: &str = "observations.jsonl";

/// 温度校准 artifact 文件名（{scoring_dir}/calibration.json）。
pub(crate) const CALIBRATION_FILE: &str = "calibration.json";

/// 评分审计事件文件名（{scoring_dir}/audit.jsonl）。
pub(crate) const AUDIT_FILE: &str = "audit.jsonl";

/// 本构建是否提供 embedded 生产装配（feature `embedded` 编译开关）。
///
/// composition 据此在装配前判定 EmbeddedUnavailable（typed startup outcome），
/// 保证「feature 关闭」路径连 manifest 都不解析、也不链接 llama.cpp。
pub const EMBEDDED_SCORING_AVAILABLE: bool = cfg!(feature = "embedded");

// --- System One 模型 manifest 契约（来源：docs/design/02-modules/systemone/01-systemone-scoring.md §4.2）---

/// manifest 当前 schema 版本。
pub(crate) const MODEL_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// 生产模型 hidden size（kev 决策位口径，manifest 锁定）。
pub(crate) const MODEL_HIDDEN_SIZE: u32 = 1024;

/// 生产 PointerHead 维度（kev 决策位口径，manifest 锁定）。
pub(crate) const MODEL_POINTER_DIMENSION: u32 = 256;

/// 首批必选平台标识；支持平台列表 MUST 至少包含它。
pub(crate) const MODEL_REQUIRED_PLATFORM: &str = "macos-aarch64";

/// engine_revision 作为安装目录段的最大长度（canonical segment 规则）。
pub(crate) const MODEL_ENGINE_REVISION_MAX_LEN: usize = 64;

/// 生产 GGUF 权重资产路径（相对 manifest 根目录）。
pub(crate) const MODEL_GGUF_FILE_NAME: &str = "model.gguf";

/// 生产 PointerHead 权重资产路径（相对 manifest 根目录）。
pub(crate) const POINTER_HEAD_FILE_NAME: &str = "pointer_head.safetensors";

/// tokenizer 资产目录前缀（其下至少一个文件）。
pub(crate) const TOKENIZER_DIR_PREFIX: &str = "tokenizer/";

// --- System One 模型本地存储（来源：adapters/model_assets.rs，§4.2 原子安装）---

/// revision 安装目录内的 canonical manifest 文件名。
pub(crate) const MODEL_MANIFEST_FILE_NAME: &str = "manifest.json";

/// 暂存目录隐藏前缀：runtime 只解析 canonical revision 目录名，
/// canonical segment 规则要求首字符为字母数字，`.tmp-` 永不冲突。
pub(crate) const STAGING_DIR_PREFIX: &str = ".tmp-";

/// 单次安装创建暂存目录的最大命名冲突重试次数（pid + 原子序号仍撞名时失败）。
pub(crate) const STAGING_ATTEMPT_LIMIT: usize = 128;

// --- System One 模型 HTTP 抓取（来源：adapters/fetch_http.rs，§4.2 手动下载）---

/// 手动重定向的最大跟随跳数（逐跳仍校验 https；超过即 fail closed，禁止无界跳转）。
pub(crate) const MAX_REDIRECT_HOPS: usize = 5;

/// 手动下载的连接 / 单次读取空闲超时（不是下载总预算——775MB 资产总时长
/// 不受该值限制，卡死的连接才被切断）。
pub(crate) const DOWNLOAD_CONNECT_READ_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);

// --- embedded llama.cpp 评分（来源：kev.serve 上下文口径与 eval/system-one/REPORT.md 基线）---

/// kev 编码 state 上限（`SERVE_MAX_STATE`，含 state 分隔 token；超出即截断）。
#[cfg(feature = "embedded")]
pub(crate) const KEV_MAX_STATE_TOKENS: usize = 65536;

/// kev 单题 row 上限（`SERVE_MAX_BRANCH` = state 上限 + 8192；超出即拒绝）。
#[cfg(feature = "embedded")]
pub(crate) const KEV_MAX_BRANCH_TOKENS: usize = 73728;

/// embedded worker 的上下文 / 批容量（REPORT 的 llama-server 基线 `--ctx-size 16384`，
/// real case row 最大已观测 ~1.5k tokens）。
#[cfg(feature = "embedded")]
pub(crate) const EMBEDDED_CONTEXT_TOKENS: u32 = 16384;

/// 单次计算 chunk（ubatch）容量。
///
/// REPORT 基线的 `--ubatch-size 16384` 在 16GiB Apple Silicon 上由当前 llama.cpp
/// 预留最坏情况 Metal 计算缓冲（实测 ≈15.5GiB）必然 OOM，故收窄为 2048
///（实测 ≈2GiB）；超长 row 由 llama.cpp 按 ubatch 切分解码，batch 上限仍为
/// `EMBEDDED_CONTEXT_TOKENS`。
#[cfg(feature = "embedded")]
pub(crate) const EMBEDDED_UBATCH_TOKENS: u32 = 2048;

/// 安装目录内 HF fast tokenizer 的相对路径（kev causal row 编码输入）。
#[cfg(feature = "embedded")]
pub(crate) const TOKENIZER_JSON_RELATIVE_PATH: &str = "tokenizer/tokenizer.json";

/// PointerHead 张量 shape 生成器：入参 (pointer_dimension, hidden_size)。
#[cfg(feature = "embedded")]
pub(crate) type PointerHeadShapeFn = fn(usize, usize) -> Vec<u64>;

/// PointerHead safetensors 四个张量的固定名称与 shape 生成器。
#[cfg(feature = "embedded")]
pub(crate) const POINTER_HEAD_TENSORS: [(&str, PointerHeadShapeFn); 4] = [
    ("q.weight", |pointer_dimension, hidden_size| {
        vec![pointer_dimension as u64, hidden_size as u64]
    }),
    ("q.bias", |pointer_dimension, _| {
        vec![pointer_dimension as u64]
    }),
    ("k.weight", |pointer_dimension, hidden_size| {
        vec![pointer_dimension as u64, hidden_size as u64]
    }),
    ("k.bias", |pointer_dimension, _| {
        vec![pointer_dimension as u64]
    }),
];
