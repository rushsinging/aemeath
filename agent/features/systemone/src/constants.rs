pub(crate) const LOG_TARGET: &str = "aemeath:agent:systemone";

/// Jev 评分端点路径（拼在配置的 base URL 后）。
pub(crate) const SYSTEMONE_PATH: &str = "/v1/systemone";

/// 连接预检窗口：规避 hyper-util 对 reusable body 的 connect 重试循环
///（实测 connect refused 等满总超时才失败）。
pub(crate) const PREFLIGHT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// 校准观测记录文件名（{scoring_dir}/observations.jsonl）。
pub(crate) const OBSERVATIONS_FILE: &str = "observations.jsonl";

/// 温度校准 artifact 文件名（{scoring_dir}/calibration.json）。
pub(crate) const CALIBRATION_FILE: &str = "calibration.json";

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
