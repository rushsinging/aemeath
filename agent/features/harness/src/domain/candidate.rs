//! RSI 待审候选：需要用户确认的全局经验变更。
//!
//! 候选是“提案”而非生效规则；只有 `aemeath rsi review approve` 才写入组件。

use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use share::ids::RsiCandidateId;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Pending,
    Active,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateConflict {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RsiCandidate {
    pub id: RsiCandidateId,
    pub scope: CandidateScope,
    pub target_component: String,
    pub base_body: String,
    pub proposed_body: String,
    pub evidence: Vec<String>,
    pub projects: Vec<String>,
    pub risk: CandidateRisk,
    #[serde(default)]
    pub conflict: Option<CandidateConflict>,
    pub status: CandidateStatus,
    pub created_at_secs: u64,
    #[serde(default)]
    pub reviewed_at_secs: Option<u64>,
    #[serde(default)]
    pub review_reason: Option<String>,
}

impl RsiCandidate {
    pub fn new_pending(
        scope: CandidateScope,
        target_component: String,
        base_body: String,
        proposed_body: String,
        evidence: Vec<String>,
        projects: Vec<String>,
        risk: CandidateRisk,
    ) -> Self {
        Self {
            id: RsiCandidateId::new_v7(),
            scope,
            target_component,
            base_body,
            proposed_body,
            evidence,
            projects,
            risk,
            conflict: None,
            status: CandidateStatus::Pending,
            created_at_secs: now_secs(),
            reviewed_at_secs: None,
            review_reason: None,
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// JSONL 候选库（append-only 历史；状态更新时整体原子重写）。
#[derive(Debug, Clone)]
pub struct RsiCandidateStore {
    path: PathBuf,
}

impl RsiCandidateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<RsiCandidate>, HarnessError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("读取候选库失败：{error}"),
                ))
            }
        };
        let mut candidates = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(candidate) = serde_json::from_str::<RsiCandidate>(line) {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }

    pub fn pending(&self) -> Result<Vec<RsiCandidate>, HarnessError> {
        Ok(self
            .load()?
            .into_iter()
            .filter(|candidate| candidate.status == CandidateStatus::Pending)
            .collect())
    }

    pub fn find(&self, id: &str) -> Result<Option<RsiCandidate>, HarnessError> {
        let parsed = RsiCandidateId::parse(id).ok();
        Ok(self.load()?.into_iter().find(|candidate| {
            candidate.id.as_str() == id
                || parsed
                    .as_ref()
                    .is_some_and(|parsed| &candidate.id == parsed)
        }))
    }

    pub fn append(&self, candidate: &RsiCandidate) -> Result<(), HarnessError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建候选库目录失败：{error}"))
            })?;
        }
        let line = serde_json::to_string(candidate).map_err(|error| {
            HarnessError::invalid(ERROR_DOMAIN, format!("候选序列化失败：{error}"))
        })?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("打开候选库失败：{error}"))
            })?;
        writeln!(file, "{line}")
            .map_err(|error| HarnessError::storage(ERROR_DOMAIN, format!("写候选库失败：{error}")))
    }

    pub fn update(
        &self,
        id: &str,
        status: CandidateStatus,
        reason: Option<String>,
    ) -> Result<RsiCandidate, HarnessError> {
        let mut candidates = self.load()?;
        let Some(candidate) = candidates
            .iter_mut()
            .find(|candidate| candidate.id.as_str() == id)
        else {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("候选不存在：{id}"),
            ));
        };
        candidate.status = status;
        candidate.reviewed_at_secs = Some(now_secs());
        candidate.review_reason = reason;
        let updated = candidate.clone();
        let mut text = String::new();
        for candidate in &candidates {
            text.push_str(&serde_json::to_string(candidate).map_err(|error| {
                HarnessError::invalid(ERROR_DOMAIN, format!("候选序列化失败：{error}"))
            })?);
            text.push('\n');
        }
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, text).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写候选临时文件失败：{error}"))
        })?;
        std::fs::rename(&temp, &self.path).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("替换候选库失败：{error}"))
        })?;
        Ok(updated)
    }
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
