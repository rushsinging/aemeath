//! 审计装饰器：每次评分决策落审计事件到 `audit.jsonl`。
//!
//! 事件携带：时间戳、引擎 revision、prompt sha256、probabilities、校准级别、
//! 延迟与结果分类（吸收 semif 的溯源要求）。审计落盘失败 NEVER 影响评分返回。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::adapters::calibration_store::append_jsonl_line_sync;
use crate::domain::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable,
};
use crate::ports::ScoringPort;

/// 评分审计事件（一行 JSONL）。
#[derive(Debug, Clone, Serialize)]
pub struct ScoringAuditEvent {
    pub timestamp: String,
    pub engine_revision: String,
    pub prompt_sha256: String,
    pub question_count: usize,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_kind: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub probabilities: Vec<Vec<f64>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calibration: Vec<CalibrationLevel>,
    pub latency_ms: u128,
}

/// 审计装饰器：透传评分结果，同时落审计事件（best-effort）。
pub struct AuditedScoringAdapter {
    inner: Arc<dyn ScoringPort>,
    engine_revision: String,
    audit_path: PathBuf,
    clock: Arc<dyn Fn() -> String + Send + Sync>,
}

impl AuditedScoringAdapter {
    /// `clock` 注入时间源（生产：RFC3339 系统时钟；测试：固定值）。
    pub fn new(
        inner: Arc<dyn ScoringPort>,
        engine_revision: impl Into<String>,
        audit_path: PathBuf,
        clock: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            engine_revision: engine_revision.into(),
            audit_path,
            clock,
        }
    }
}

/// prompt 指纹：state + 题型结构序列化的 sha256（与实际发送内容同源）。
fn prompt_fingerprint(state: &ScoringState, questions: &[ScoringQuestion]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(state.as_str().as_bytes());
    for question in questions {
        if let Ok(serialized) = serde_json::to_vec(question) {
            hasher.update(serialized);
        }
    }
    format!("{:x}", hasher.finalize())
}

fn answer_probabilities(answer: &ScoringAnswer) -> Vec<f64> {
    match answer {
        ScoringAnswer::Noul { p_true, .. } => vec![*p_true],
        ScoringAnswer::Choice { probabilities, .. } => {
            probabilities.iter().map(|(_, p)| *p).collect()
        }
        ScoringAnswer::Score { probabilities, .. } => probabilities.clone(),
    }
}

#[async_trait]
impl ScoringPort for AuditedScoringAdapter {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let started = Instant::now();
        let outcome = self.inner.answer(state, questions).await;
        let latency_ms = started.elapsed().as_millis();

        let event = match &outcome {
            Ok(answers) => ScoringAuditEvent {
                timestamp: (self.clock)(),
                engine_revision: self.engine_revision.clone(),
                prompt_sha256: prompt_fingerprint(state, questions),
                question_count: questions.len(),
                outcome: "ok",
                unavailable_kind: None,
                probabilities: answers.iter().map(answer_probabilities).collect(),
                calibration: answers.iter().map(|answer| answer.calibration()).collect(),
                latency_ms,
            },
            Err(error) => ScoringAuditEvent {
                timestamp: (self.clock)(),
                engine_revision: self.engine_revision.clone(),
                prompt_sha256: prompt_fingerprint(state, questions),
                question_count: questions.len(),
                outcome: "unavailable",
                unavailable_kind: Some(format!("{:?}", error.kind())),
                probabilities: Vec::new(),
                calibration: Vec::new(),
                latency_ms,
            },
        };
        write_audit_event(&self.audit_path, &event).await;

        outcome
    }
}

async fn write_audit_event(path: &std::path::Path, event: &ScoringAuditEvent) {
    let mut line = match serde_json::to_string(event) {
        Ok(serialized) => serialized,
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "scoring_audit_encode_failed error={error}"
            );
            return;
        }
    };
    line.push('\n');
    let path = path.to_path_buf();
    let outcome = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        append_jsonl_line_sync(&path, &line)
    })
    .await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "scoring_audit_append_failed error={error}"
            );
        }
        Err(join_error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "scoring_audit_join_failed error={join_error}"
            );
        }
    }
}

#[cfg(test)]
#[path = "audited_tests.rs"]
mod tests;
