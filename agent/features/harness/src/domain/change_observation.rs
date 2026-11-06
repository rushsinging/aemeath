//! RSI 自动观察：每次写入生成 change，后续周期验收，失败自动回滚。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use crate::domain::evidence::EvidenceBundle;
use crate::domain::versioning::{rollback_component, ComponentVersionStore};
use serde::{Deserialize, Serialize};
use share::ids::ChangeObservationId;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Observing,
    Kept,
    RolledBack,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeObservation {
    pub id: ChangeObservationId,
    pub component: String,
    pub snapshot_cycle: String,
    pub before_hash: String,
    pub after_hash: String,
    pub target_signal_ids: Vec<String>,
    pub baseline_signal_ids: Vec<String>,
    pub observation_cycles: u32,
    pub status: ObservationStatus,
    pub created_at_secs: u64,
    pub last_evaluated_at_secs: Option<u64>,
    pub rollback_reason: Option<String>,
}

impl ChangeObservation {
    pub fn new_applied(
        component: String,
        snapshot_cycle: String,
        before_hash: String,
        after_hash: String,
        target_signal_ids: Vec<String>,
        baseline_signal_ids: Vec<String>,
    ) -> Self {
        Self {
            id: ChangeObservationId::new_v7(),
            component,
            snapshot_cycle,
            before_hash,
            after_hash,
            target_signal_ids,
            baseline_signal_ids,
            observation_cycles: 0,
            status: ObservationStatus::Observing,
            created_at_secs: now_secs(),
            last_evaluated_at_secs: None,
            rollback_reason: None,
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn body_hash(body: &str) -> String {
    utils::stable_sha256_hex(b"aemeath.harness.change.v1\0", &[body.as_bytes()])
}

#[derive(Debug, Clone)]
pub struct ChangeObservationStore {
    path: PathBuf,
}

impl ChangeObservationStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<ChangeObservation>, HarnessError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("读取 change 观察库失败：{error}"),
                ))
            }
        };
        let mut values = Vec::new();
        for line in text.lines() {
            if let Ok(value) = serde_json::from_str::<ChangeObservation>(line) {
                values.push(value);
            }
        }
        Ok(values)
    }

    pub fn observing(&self) -> Result<Vec<ChangeObservation>, HarnessError> {
        Ok(self
            .load()?
            .into_iter()
            .filter(|value| value.status == ObservationStatus::Observing)
            .collect())
    }

    pub fn append(&self, value: &ChangeObservation) -> Result<(), HarnessError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建 change 目录失败：{error}"))
            })?;
        }
        let line = serde_json::to_string(value).map_err(|error| {
            HarnessError::invalid(ERROR_DOMAIN, format!("change 序列化失败：{error}"))
        })?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("打开 change 库失败：{error}"))
            })?;
        writeln!(file, "{line}").map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写 change 库失败：{error}"))
        })
    }

    pub fn save_all(&self, values: &[ChangeObservation]) -> Result<(), HarnessError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建 change 目录失败：{error}"))
            })?;
        }
        let mut text = String::new();
        for value in values {
            text.push_str(&serde_json::to_string(value).map_err(|error| {
                HarnessError::invalid(ERROR_DOMAIN, format!("change 序列化失败：{error}"))
            })?);
            text.push('\n');
        }
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, text).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写 change 临时文件失败：{error}"))
        })?;
        std::fs::rename(&temp, &self.path).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("替换 change 库失败：{error}"))
        })
    }
}

fn signal_tool(signal_id: &str) -> &str {
    signal_id.split(':').nth(1).unwrap_or("")
}

fn relevant_activity(observation: &ChangeObservation, evidence: &EvidenceBundle) -> bool {
    let tools: Vec<&str> = observation
        .target_signal_ids
        .iter()
        .map(|signal| signal_tool(signal))
        .filter(|tool| !tool.is_empty())
        .collect();
    evidence
        .signals
        .iter()
        .any(|signal| tools.contains(&signal_tool(&signal.signal_id)))
}

fn target_reappeared(observation: &ChangeObservation, evidence: &EvidenceBundle) -> bool {
    evidence
        .signals
        .iter()
        .any(|signal| observation.target_signal_ids.contains(&signal.signal_id))
}

/// 评估并回滚；返回可读摘要。失败只返回错误摘要，不阻断主周期。
pub fn evaluate_and_rollback(
    store: &ChangeObservationStore,
    registry: &mut dyn ComponentRegistry,
    version_store: &ComponentVersionStore,
    evidence: &EvidenceBundle,
) -> Vec<String> {
    let mut values = match store.load() {
        Ok(values) => values,
        Err(error) => return vec![format!("变更观察读取失败（跳过）：{}", error.message())],
    };
    let mut summaries = Vec::new();
    for value in values
        .iter_mut()
        .filter(|value| value.status == ObservationStatus::Observing)
    {
        value.last_evaluated_at_secs = Some(now_secs());
        value.observation_cycles = value.observation_cycles.saturating_add(1);
        if target_reappeared(value, evidence) {
            let Some(id) = crate::parse_component_id(&value.component) else {
                value.status = ObservationStatus::Stale;
                value.rollback_reason = Some("组件标识无效".to_string());
                continue;
            };
            let current = match registry.read(&id) {
                Ok(content) => content,
                Err(error) => {
                    value.status = ObservationStatus::Stale;
                    value.rollback_reason = Some(format!("读取组件失败：{}", error.message()));
                    continue;
                }
            };
            if body_hash(&current.body) != value.after_hash {
                value.status = ObservationStatus::Stale;
                value.rollback_reason = Some("组件已被外部修改，未自动回滚".to_string());
                continue;
            }
            match version_store.load(&id, Some(&value.snapshot_cycle)) {
                Ok(snapshot) => match rollback_component(registry, &id, &snapshot) {
                    Ok(_) => {
                        value.status = ObservationStatus::RolledBack;
                        value.rollback_reason = Some("目标 signal 再次出现".to_string());
                        summaries.push(format!(
                            "变更观察：{} 目标 signal 复现，已自动回滚（{}）",
                            value.component, value.id
                        ));
                    }
                    Err(error) => {
                        value.rollback_reason = Some(format!("回滚失败：{}", error.message()));
                        summaries.push(format!("变更观察：{} 回滚失败", value.component));
                    }
                },
                Err(error) => {
                    value.rollback_reason = Some(format!("快照缺失：{}", error.message()));
                    summaries.push(format!("变更观察：{} 快照缺失，无法回滚", value.component));
                }
            }
        } else if evidence.signals.is_empty() {
            // 无证据时无法判断，保持 observing。
        } else if relevant_activity(value, evidence) && value.observation_cycles >= 2 {
            value.status = ObservationStatus::Kept;
            summaries.push(format!(
                "变更观察：{} 目标 signal 未复现，保留",
                value.component
            ));
        } else if value.observation_cycles >= 5 {
            value.status = ObservationStatus::Stale;
            value.rollback_reason = Some("连续多周期无相关活动，停止观察".to_string());
        }
    }
    if let Err(error) = store.save_all(&values) {
        summaries.push(format!("变更观察保存失败：{}", error.message()));
    }
    summaries
}

#[cfg(test)]
#[path = "change_observation_tests.rs"]
mod tests;
