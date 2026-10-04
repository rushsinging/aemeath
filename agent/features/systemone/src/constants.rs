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
