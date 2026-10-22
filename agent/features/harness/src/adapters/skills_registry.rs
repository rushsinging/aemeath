//! skills 目录 registry（c5）：`skills/<name>/SKILL.md` 扫描枚举 +
//! YAML frontmatter 校验 + 原子写（#2018）。

use crate::adapters::filesystem::parse_markdown_with_frontmatter;
use crate::application::ComponentRegistry;
use crate::constants::{ERROR_DOMAIN, SKILL_MAX_BYTES};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use std::path::{Path, PathBuf};

/// skills 目录组件注册表：每 run 扫描枚举（天然热载），
/// ComponentId = { Skill, skill 目录名 }。
pub struct SkillsDirectoryRegistry {
    directory: PathBuf,
}

impl SkillsDirectoryRegistry {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn skill_path(&self, name: &ComponentName) -> PathBuf {
        self.directory.join(name.as_str()).join("SKILL.md")
    }

    fn skill_id(name: &str) -> ComponentId {
        ComponentId {
            kind: ComponentKind::Skill,
            name: ComponentName::new(name),
        }
    }

    fn validate_skill_source(source: &str) -> Result<(), HarnessError> {
        if source.len() as u64 > SKILL_MAX_BYTES {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("SKILL.md 超过大小上限 {SKILL_MAX_BYTES} 字节"),
            ));
        }
        let content = parse_markdown_with_frontmatter(source)?;
        let Some(frontmatter) = content.frontmatter else {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                "SKILL.md 缺少 YAML frontmatter（--- 定界）",
            ));
        };
        let description = frontmatter
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if description.trim().is_empty() {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                "SKILL.md frontmatter 缺少非空 description",
            ));
        }
        Ok(())
    }

    /// 原子写（tmp+rename，与 filesystem adapter 同约定）。
    fn atomic_write(path: &Path, text: &str) -> Result<(), HarnessError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建目录失败：{error}"))
            })?;
        }
        let temp_path = path.with_extension("md.tmp");
        std::fs::write(&temp_path, text).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写临时文件失败：{error}"))
        })?;
        std::fs::rename(&temp_path, path).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("原子替换失败：{error}"))
        })?;
        Ok(())
    }
}

impl ComponentRegistry for SkillsDirectoryRegistry {
    fn handles(&self, id: &ComponentId) -> bool {
        id.kind == ComponentKind::Skill
    }

    fn list(&self) -> Vec<ComponentId> {
        let Ok(entries) = std::fs::read_dir(&self.directory) else {
            return Vec::new();
        };
        let mut ids: Vec<ComponentId> = entries
            .flatten()
            .filter(|entry| entry.path().join("SKILL.md").is_file())
            .filter_map(|entry| entry.file_name().to_str().map(Self::skill_id))
            .collect();
        ids.sort();
        ids
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        if !self.handles(id) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("SkillsDirectoryRegistry 只服务 Skill 组件：{id}"),
            ));
        }
        let path = self.skill_path(&id.name);
        let source = std::fs::read_to_string(&path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取 SKILL.md 失败 {}: {error}", path.display()),
            )
        })?;
        parse_markdown_with_frontmatter(&source)
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        if !self.handles(id) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("SkillsDirectoryRegistry 只服务 Skill 组件：{id}"),
            ));
        }
        Self::validate_skill_source(&content.body)
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.validate(id, content)?;
        let path = self.skill_path(&id.name);
        Self::atomic_write(&path, &content.body)
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        let skill_dir = self.directory.join(id.name.as_str());
        match std::fs::remove_dir_all(&skill_dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("删除 skill 目录失败 {}: {error}", skill_dir.display()),
            )),
        }
    }
}
