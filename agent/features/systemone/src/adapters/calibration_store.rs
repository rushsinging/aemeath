//! 校准存储：observe 回路落盘 + 温度 artifact 加载。
//!
//! - 观测记录 append 到 `{directory}/observations.jsonl`；
//! - 温度 artifact 从 `{directory}/calibration.json` 构造时加载一次（离线拟合，重启生效）；
//! - artifact 缺失或温度非法时按 `CalibrationLevel::Raw` 处理，NEVER panic。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::CalibrationLevel;
use crate::ports::{CalibrationObservation, CalibrationPort};

const OBSERVATIONS_FILE: &str = "observations.jsonl";
const CALIBRATION_FILE: &str = "calibration.json";

/// 温度校准 artifact：离线拟合产物的最小形态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationArtifact {
    temperature: f64,
}

impl CalibrationArtifact {
    pub fn temperature(&self) -> f64 {
        self.temperature
    }
}

#[derive(Debug, Deserialize)]
struct CalibrationFileSchema {
    temperature: f64,
}

/// 校准存储 adapter：JSONL 落盘 + artifact 加载。
#[derive(Debug)]
pub struct CalibrationStore {
    directory: PathBuf,
    artifact: Option<CalibrationArtifact>,
}

impl CalibrationStore {
    /// 构造并加载 artifact（一次性）；`directory` 不存在时 observe 时创建。
    pub fn new(directory: PathBuf) -> Self {
        let artifact = load_artifact(&directory);
        Self {
            directory,
            artifact,
        }
    }

    /// 当前生效的温度 artifact；无则 `None`（即 Raw）。
    pub fn artifact(&self) -> Option<CalibrationArtifact> {
        self.artifact
    }
}

fn load_artifact(directory: &Path) -> Option<CalibrationArtifact> {
    let path = directory.join(CALIBRATION_FILE);
    let source = std::fs::read_to_string(&path).ok()?;
    let parsed: CalibrationFileSchema = serde_json::from_str(&source)
        .map_err(|error| {
            log::warn!(
                target: crate::LOG_TARGET,
                "calibration_artifact_invalid path={} error={error}",
                path.display()
            );
            error
        })
        .ok()?;
    if !parsed.temperature.is_finite() || parsed.temperature <= 0.0 {
        log::warn!(
            target: crate::LOG_TARGET,
            "calibration_artifact_invalid path={} reason=temperature_out_of_range value={}",
            path.display(),
            parsed.temperature
        );
        return None;
    }
    Some(CalibrationArtifact {
        temperature: parsed.temperature,
    })
}

#[async_trait]
impl CalibrationPort for CalibrationStore {
    async fn observe(&self, record: CalibrationObservation) {
        if let Err(error) = append_observation(&self.directory, &record).await {
            log::warn!(
                target: crate::LOG_TARGET,
                "calibration_observe_failed error={error}"
            );
        }
    }

    fn current(&self) -> CalibrationLevel {
        match self.artifact {
            Some(_) => CalibrationLevel::Temperature,
            None => CalibrationLevel::Raw,
        }
    }
}

async fn append_observation(
    directory: &Path,
    record: &CalibrationObservation,
) -> std::io::Result<()> {
    let mut line = serde_json::to_string(record)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    line.push('\n');
    let directory = directory.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        append_jsonl_line_sync(&directory.join(OBSERVATIONS_FILE), &line)
    })
    .await?
}

/// 同步追加一行 JSONL（供 crate 内落盘路径复用）。
///
/// 同步 write + 同步 close（随 File drop）一次完成：tokio::fs::File 的 close
/// 延迟到 blocking 池异步执行，fd 复用窗口下存在 write 成功但数据落到复用 fd
/// 的实测风险（并行测试环境复现），故 NEVER 改用 tokio::fs 写路径。
pub(crate) fn append_jsonl_line_sync(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

#[cfg(test)]
#[path = "calibration_store_tests.rs"]
mod tests;
