//! RSI 自动观察：每次写入生成 change，后续周期验收，失败自动回滚。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use crate::domain::evidence::EvidenceBundle;
use crate::domain::improvement::{complete_with_usage_event, RsiLlmPort};
use crate::domain::versioning::{rollback_component, ComponentVersionStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use share::ids::ChangeObservationId;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Observing,
    Kept,
    RolledBack,
    /// 被同一组件的后续 RSI/Compose 写入覆盖。
    Superseded,
    /// 组件被外部修改，无法安全判断。
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ObservationVerdict {
    Keep,
    Rollback,
    Observe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeObservation {
    pub id: ChangeObservationId,
    pub component: String,
    pub snapshot_cycle: String,
    pub before_hash: String,
    pub after_hash: String,
    /// 脱敏后的展示用 signal id。
    pub target_signal_ids: Vec<String>,
    /// signal id 的稳定 hash，用于匹配；不保存原始命令。
    #[serde(default)]
    pub target_signal_hashes: Vec<String>,
    /// 脱敏后的 baseline 展示。
    pub baseline_signal_ids: Vec<String>,
    /// 写入时已经处理过的 run；下一轮只有新 run 才能作为复现证据。
    #[serde(default)]
    pub baseline_run_ids: Vec<String>,
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
        baseline_run_ids: Vec<String>,
    ) -> Self {
        let target_signal_hashes = target_signal_ids
            .iter()
            .map(|signal| signal_hash(signal))
            .collect();
        let target_signal_ids = target_signal_ids
            .iter()
            .map(|signal| display_signal_id(signal))
            .collect();
        let baseline_signal_ids = baseline_signal_ids
            .iter()
            .map(|signal| display_signal_id(signal))
            .collect();
        Self {
            id: ChangeObservationId::new_v7(),
            component,
            snapshot_cycle,
            before_hash,
            after_hash,
            target_signal_ids,
            target_signal_hashes,
            baseline_signal_ids,
            baseline_run_ids,
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

fn signal_hash(signal_id: &str) -> String {
    utils::stable_sha256_hex(b"aemeath.harness.signal-id.v1\0", &[signal_id.as_bytes()])
}

fn display_signal_id(signal_id: &str) -> String {
    let mut parts = signal_id.split(':');
    match (parts.next(), parts.next()) {
        (Some(kind), Some(tool)) => format!("{kind}:{tool}"),
        _ => crate::domain::redaction::redact_sensitive_text(signal_id),
    }
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

    /// 同一组件出现新写入时，之前仍在观察的记录全部标记 superseded。
    pub fn supersede_component(
        &self,
        component: &str,
        reason: impl Into<String>,
    ) -> Result<usize, HarnessError> {
        let mut values = self.load()?;
        let reason = reason.into();
        let mut count = 0;
        for value in values.iter_mut() {
            if value.component == component && value.status == ObservationStatus::Observing {
                value.status = ObservationStatus::Superseded;
                value.last_evaluated_at_secs = Some(now_secs());
                value.rollback_reason = Some(reason.clone());
                count += 1;
            }
        }
        if count > 0 {
            self.save_all(&values)?;
        }
        Ok(count)
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

fn evidence_from_new_runs(
    observation: &ChangeObservation,
    evidence: &EvidenceBundle,
) -> EvidenceBundle {
    if observation.baseline_run_ids.is_empty() {
        // legacy 记录没有 run waterline：无法区分历史与新证据，保持安全不裁决。
        return EvidenceBundle::default();
    }
    let mut fresh = EvidenceBundle {
        sessions_covered: 0,
        signals: Vec::new(),
    };
    for signal in &evidence.signals {
        let sources: Vec<_> = signal
            .sources
            .iter()
            .filter(|source| !observation.baseline_run_ids.contains(&source.run_id))
            .cloned()
            .collect();
        if sources.is_empty() {
            continue;
        }
        let mut signal = signal.clone();
        signal.sources = sources;
        fresh.signals.push(signal);
        fresh.sessions_covered += 1;
    }
    fresh
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
    if observation.target_signal_hashes.is_empty() {
        // legacy 记录：旧字段保存的是原始 id。
        return evidence
            .signals
            .iter()
            .any(|signal| observation.target_signal_ids.contains(&signal.signal_id));
    }
    evidence.signals.iter().any(|signal| {
        let hash = signal_hash(&signal.signal_id);
        observation.target_signal_hashes.contains(&hash)
    })
}

/// 评估并回滚；返回可读摘要。失败只返回错误摘要，不阻断主周期。
pub fn evaluate_and_rollback(
    store: &ChangeObservationStore,
    registry: &mut dyn ComponentRegistry,
    version_store: &ComponentVersionStore,
    evidence: &EvidenceBundle,
    lesson_store: Option<&crate::domain::lesson::LessonStore>,
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
        let fresh = evidence_from_new_runs(value, evidence);
        let evidence = &fresh;
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
                if has_newer_snapshot(version_store, &id, &value.snapshot_cycle) {
                    value.status = ObservationStatus::Superseded;
                    value.rollback_reason = Some("被后续版本写入覆盖".to_string());
                } else {
                    value.status = ObservationStatus::Stale;
                    value.rollback_reason = Some("组件已被外部修改，未自动回滚".to_string());
                }
                continue;
            }
            match version_store.load(&id, Some(&value.snapshot_cycle)) {
                Ok(snapshot) => match rollback_component(registry, &id, &snapshot) {
                    Ok(_) => {
                        value.status = ObservationStatus::RolledBack;
                        value.rollback_reason = Some("目标 signal 再次出现".to_string());
                        if let Some(lesson_store) = lesson_store {
                            let _ = lesson_store.retire_for_component(&id);
                        }
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

fn has_newer_snapshot(
    version_store: &ComponentVersionStore,
    id: &crate::domain::component::ComponentId,
    snapshot_cycle: &str,
) -> bool {
    version_store
        .history(id)
        .ok()
        .and_then(|history| history.into_iter().next())
        .is_some_and(|latest| latest.cycle_id.as_str() > snapshot_cycle)
}

fn observation_verdict_prompt(
    observation: &ChangeObservation,
    evidence: &EvidenceBundle,
) -> String {
    let mut signals = Vec::new();
    for signal in &evidence.signals {
        signals.push(format!(
            "- {} | kind={} | occurrences={} | summary={}",
            crate::domain::redaction::redact_sensitive_text(&signal.signal_id),
            signal.kind.as_str(),
            signal.occurrences,
            crate::domain::redaction::redact_sensitive_text(&signal.summary)
        ));
    }
    format!(
        r#"你是 RSI 变更观察裁决器。判断这次组件变更是否有效。

变更：
- id: {id}
- component: {component}
- target_signal_ids: {targets}
- observation_cycles: {cycles}
- baseline_signal_ids: {baseline}

当前证据：
{signals}

判断规则：
- target signal 再次出现 → rollback
- 目标 signal 未出现且有相关活动，并且变更语义合理 → keep
- 证据不足以判断 → observe
- 变更明显引入新失败或语义错误 → rollback

只输出 JSON：
{{"decision":"keep|rollback|observe","reason":"<一句话>"}}"#,
        id = observation.id,
        component = observation.component,
        targets = observation.target_signal_ids.join(", "),
        cycles = observation.observation_cycles,
        baseline = observation.baseline_signal_ids.join(", "),
        signals = if signals.is_empty() {
            "（无）".to_string()
        } else {
            signals.join("\n")
        },
    )
}

async fn judge_with_llm(
    llm: &dyn RsiLlmPort,
    observation: &ChangeObservation,
    evidence: &EvidenceBundle,
) -> Option<ObservationVerdict> {
    let prompt = observation_verdict_prompt(observation, evidence);
    let completion = complete_with_usage_event(llm, &prompt, "change_observation", 0, None)
        .await
        .ok()?;
    let start = completion.text.find('{')?;
    let source = completion.text.get(start..)?;
    let value: Value = serde_json::from_str(source).ok()?;
    serde_json::from_value(value.get("decision")?.clone()).ok()
}

/// 带 LLM 语义裁决的观察入口；LLM 不可用时回退确定性规则。
pub async fn evaluate_and_rollback_with_llm(
    store: &ChangeObservationStore,
    registry: &mut dyn ComponentRegistry,
    version_store: &ComponentVersionStore,
    evidence: &EvidenceBundle,
    llm: &dyn RsiLlmPort,
    lesson_store: Option<&crate::domain::lesson::LessonStore>,
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
        let fresh = evidence_from_new_runs(value, evidence);
        let evidence = &fresh;
        let target_present = target_reappeared(value, evidence);
        let rollback = if target_present {
            true
        } else if evidence.signals.is_empty() {
            false
        } else if relevant_activity(value, evidence) && value.observation_cycles >= 2 {
            match judge_with_llm(llm, value, evidence).await {
                Some(ObservationVerdict::Rollback) => true,
                Some(ObservationVerdict::Keep) => {
                    value.status = ObservationStatus::Kept;
                    summaries.push(format!("变更观察：{} LLM 判定有效，保留", value.component));
                    false
                }
                Some(ObservationVerdict::Observe) | None => false,
            }
        } else {
            false
        };
        if rollback {
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
                if has_newer_snapshot(version_store, &id, &value.snapshot_cycle) {
                    value.status = ObservationStatus::Superseded;
                    value.rollback_reason = Some("被后续版本写入覆盖".to_string());
                } else {
                    value.status = ObservationStatus::Stale;
                    value.rollback_reason = Some("组件已被外部修改，未自动回滚".to_string());
                }
                continue;
            }
            match version_store.load(&id, Some(&value.snapshot_cycle)) {
                Ok(snapshot) => match rollback_component(registry, &id, &snapshot) {
                    Ok(_) => {
                        value.status = ObservationStatus::RolledBack;
                        value.rollback_reason = Some(if target_present {
                            "目标 signal 再次出现".to_string()
                        } else {
                            "LLM 判定变更无效".to_string()
                        });
                        if let Some(lesson_store) = lesson_store {
                            let _ = lesson_store.retire_for_component(&id);
                        }
                        summaries.push(format!(
                            "变更观察：{} 已自动回滚（{}）",
                            value.component, value.id
                        ));
                    }
                    Err(error) => {
                        value.rollback_reason = Some(format!("回滚失败：{}", error.message()));
                    }
                },
                Err(error) => {
                    value.rollback_reason = Some(format!("快照缺失：{}", error.message()));
                }
            }
        } else if !target_present && value.observation_cycles >= 5 {
            value.status = ObservationStatus::Stale;
            value.rollback_reason = Some("连续多周期无明确结论，停止观察".to_string());
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
