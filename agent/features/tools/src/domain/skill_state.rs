//! Skill 加载状态 Published Language。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "instance_id", rename_all = "snake_case")]
pub enum SkillLoadScope {
    Main,
    Subagent(String),
}

impl SkillLoadScope {
    pub const fn main() -> Self {
        Self::Main
    }

    pub fn subagent(value: impl Into<String>) -> Result<Self, SkillLoadStateError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SkillLoadStateError::InvalidInstanceId);
        }
        Ok(Self::Subagent(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoadMutation {
    session_id: String,
    scope: SkillLoadScope,
    skill_name: String,
    revision: String,
}

impl SkillLoadMutation {
    pub fn new(
        session_id: impl Into<String>,
        scope: SkillLoadScope,
        skill_name: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, SkillLoadStateError> {
        let mutation = Self {
            session_id: session_id.into(),
            scope,
            skill_name: skill_name.into(),
            revision: revision.into(),
        };
        if mutation.session_id.trim().is_empty() {
            return Err(SkillLoadStateError::InvalidSessionId);
        }
        if mutation.skill_name.trim().is_empty() {
            return Err(SkillLoadStateError::InvalidSkillName);
        }
        if mutation.revision.trim().is_empty() {
            return Err(SkillLoadStateError::InvalidRevision);
        }
        Ok(mutation)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn scope(&self) -> &SkillLoadScope {
        &self.scope
    }

    pub fn skill_name(&self) -> &str {
        &self.skill_name
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillLoadDecision {
    Fresh,
    Updated,
    AlreadyLoaded,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillLoadStateError {
    #[error("Session identity 无效")]
    InvalidSessionId,
    #[error("Sub-agent instance identity 无效")]
    InvalidInstanceId,
    #[error("Skill canonical identity 无效")]
    InvalidSkillName,
    #[error("Skill revision 无效")]
    InvalidRevision,
    #[error("Session 不存在: {0}")]
    SessionNotFound(String),
    #[error("Skill 加载状态持久化失败: {0}")]
    Storage(String),
}

#[async_trait]
pub trait SkillLoadStatePort: Send + Sync {
    async fn compare_and_record(
        &self,
        mutation: SkillLoadMutation,
    ) -> Result<SkillLoadDecision, SkillLoadStateError>;
}
