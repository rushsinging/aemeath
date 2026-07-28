//! 动态 Skill 的 Published Language（Issue #1438）。
//!
//! Catalog 只发布廉价元数据；唯一 Skill Tool 通过 [`SkillLoadPort`]
//! 在调用时按 identity 加载一个正文。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub use super::skill_ports::{SkillCatalogPort, SkillLoadPort};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSourceKind {
    ProjectClaude,
    ProjectAgents,
    Global,
    Extra,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSource {
    pub kind: SkillSourceKind,
    pub path: String,
}

impl SkillSource {
    pub fn file(kind: SkillSourceKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
        }
    }

    pub fn builtin(uri: impl Into<String>) -> Self {
        Self {
            kind: SkillSourceKind::Builtin,
            path: uri.into(),
        }
    }
}

/// Skill Catalog 的无正文投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDescriptor {
    name: String,
    description: String,
    source: SkillSource,
    aliases: Vec<String>,
    slash_command: Option<String>,
    slash_aliases: Vec<String>,
    argument_hint: Option<String>,
}

impl SkillDescriptor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        source: SkillSource,
        aliases: Vec<String>,
        slash_command: Option<String>,
        slash_aliases: Vec<String>,
        argument_hint: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            source,
            aliases,
            slash_command,
            slash_aliases,
            argument_hint,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn source(&self) -> &SkillSource {
        &self.source
    }
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }
    pub fn slash_command(&self) -> Option<&str> {
        self.slash_command.as_deref()
    }
    pub fn slash_aliases(&self) -> &[String] {
        &self.slash_aliases
    }
    pub fn argument_hint(&self) -> Option<&str> {
        self.argument_hint.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSlashRoute {
    pub skill: String,
    pub slash_command: String,
    pub aliases: Vec<String>,
    pub argument_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogSnapshot {
    pub revision: String,
    pub skills: Vec<SkillDescriptor>,
    pub slash_routes: Vec<SkillSlashRoute>,
}

impl SkillCatalogSnapshot {
    pub fn from_descriptors(descriptors: Vec<SkillDescriptor>) -> Self {
        let mut by_name = BTreeMap::new();
        for descriptor in descriptors {
            by_name.entry(descriptor.name.clone()).or_insert(descriptor);
        }
        let skills: Vec<_> = by_name.into_values().collect();
        let slash_routes = skills
            .iter()
            .filter_map(|skill| {
                Some(SkillSlashRoute {
                    skill: skill.name.clone(),
                    slash_command: skill.slash_command.clone()?,
                    aliases: skill.slash_aliases.clone(),
                    argument_hint: skill.argument_hint.clone(),
                })
            })
            .collect();
        let revision = metadata_revision(&skills);
        Self {
            revision,
            skills,
            slash_routes,
        }
    }
}

fn metadata_revision(skills: &[SkillDescriptor]) -> String {
    let mut hasher = Sha256::new();
    for skill in skills {
        for value in std::iter::once(skill.name.as_str())
            .chain(std::iter::once(skill.description.as_str()))
            .chain(skill.aliases.iter().map(String::as_str))
            .chain(skill.slash_command.iter().map(String::as_str))
            .chain(skill.slash_aliases.iter().map(String::as_str))
            .chain(skill.argument_hint.iter().map(String::as_str))
        {
            hasher.update(value.as_bytes());
            hasher.update(b"\x1f");
        }
        hasher.update(b"\xff");
    }
    hasher
        .finalize()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct SkillQuery {
    pub project_root: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
    pub available_tools: BTreeSet<String>,
}

impl SkillQuery {
    pub fn new(
        project_root: PathBuf,
        extra_dirs: Vec<PathBuf>,
        available_tools: BTreeSet<String>,
    ) -> Self {
        Self {
            project_root,
            extra_dirs,
            available_tools,
        }
    }
}

/// 调用时加载单个 Skill 的查询。
#[derive(Debug, Clone)]
pub struct SkillLoadQuery {
    identity: String,
    pub project_root: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
    pub available_tools: BTreeSet<String>,
}

impl SkillLoadQuery {
    pub fn new(
        identity: impl Into<String>,
        project_root: PathBuf,
        extra_dirs: Vec<PathBuf>,
        available_tools: BTreeSet<String>,
    ) -> Result<Self, SkillError> {
        let identity = normalize_identity(&identity.into())?;
        Ok(Self {
            identity,
            project_root,
            extra_dirs,
            available_tools,
        })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn catalog_query(&self) -> SkillQuery {
        SkillQuery::new(
            self.project_root.clone(),
            self.extra_dirs.clone(),
            self.available_tools.clone(),
        )
    }
}

fn normalize_identity(value: &str) -> Result<String, SkillError> {
    let normalized = value.trim().trim_start_matches('/').to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(SkillError::InvalidIdentity {
            identity: value.to_string(),
        });
    }
    Ok(normalized)
}

/// Skill Tool 调用时加载的单个正文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedSkill {
    name: String,
    content: String,
    source: SkillSource,
    revision: String,
}

impl LoadedSkill {
    pub fn new(
        name: impl Into<String>,
        content: impl Into<String>,
        source: SkillSource,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            source,
            revision: revision.into(),
        }
    }

    pub fn from_content(
        name: impl Into<String>,
        content: impl Into<String>,
        source: SkillSource,
    ) -> Self {
        let name = name.into();
        let content = content.into();
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(content.as_bytes());
        let digest = hasher.finalize();
        let revision: String = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self::new(name, content, source, revision)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn content(&self) -> &str {
        &self.content
    }
    pub fn source(&self) -> &SkillSource {
        &self.source
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum SkillError {
    #[error("Skill identity 无效: {identity}")]
    InvalidIdentity { identity: String },
    #[error("Skill 不存在或当前不可用: {identity}")]
    NotFound { identity: String },
    #[error("读取 Skill 文件失败: {path}: {reason}")]
    ReadFailed { path: String, reason: String },
    #[error("解析 Skill frontmatter 失败: {path}: {reason}")]
    ParseFailed { path: String, reason: String },
}

impl SkillError {
    pub fn not_found(identity: impl Into<String>) -> Self {
        Self::NotFound {
            identity: identity.into(),
        }
    }

    pub fn read_failed(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ReadFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn parse_failed(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ParseFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }
}
