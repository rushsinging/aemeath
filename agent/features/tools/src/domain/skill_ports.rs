//! 动态 Skill 的 Catalog / Load 双端口（Issue #1438）。

use async_trait::async_trait;

use super::skill_pl::{LoadedSkill, SkillDescriptor, SkillError, SkillLoadQuery, SkillQuery};

/// 返回当前可见 Skill 的廉价元数据。
pub trait SkillCatalogPort: Send + Sync {
    fn list(&self, query: SkillQuery) -> Vec<SkillDescriptor>;
}

/// 唯一 Skill Tool 调用时按 identity 加载单个正文。
#[async_trait]
pub trait SkillLoadPort: Send + Sync {
    async fn load(&self, query: SkillLoadQuery) -> Result<LoadedSkill, SkillError>;
}
