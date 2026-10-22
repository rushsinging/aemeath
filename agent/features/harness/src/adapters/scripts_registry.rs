//! scripts 目录 registry：目录扫描枚举 + 校验 + 原子写（c7 可编辑面）。

use crate::adapters::scripts::{parse_script_frontmatter, scan_scripts_directory, validate_script};
use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::errors::HarnessError;
use std::path::{Path, PathBuf};

/// scripts 目录组件注册表：每 run 扫描枚举（天然热载），
/// ComponentId = { Script, 脚本 stem }。
pub struct ScriptsDirectoryRegistry {
    directory: PathBuf,
}

impl ScriptsDirectoryRegistry {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn path_for(&self, name: &ComponentName) -> Result<PathBuf, HarnessError> {
        let stem = name.as_str();
        for candidate in ["sh", "py"] {
            let candidate_path = self.directory.join(format!("{stem}.{candidate}"));
            if candidate_path.exists() {
                return Ok(candidate_path);
            }
        }
        // 新脚本默认 .sh
        Ok(self.directory.join(format!("{stem}.sh")))
    }

    fn script_id(name: &str) -> ComponentId {
        ComponentId {
            kind: ComponentKind::Script,
            name: ComponentName::new(name),
        }
    }

    /// 原子写（tmp+rename，与 filesystem adapter 同约定）。
    fn atomic_write(path: &Path, text: &str) -> Result<(), HarnessError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建目录失败：{error}"))
            })?;
        }
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, text).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写临时文件失败：{error}"))
        })?;
        std::fs::rename(&temp_path, path).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("原子替换失败：{error}"))
        })?;
        Ok(())
    }
}

impl ComponentRegistry for ScriptsDirectoryRegistry {
    fn handles(&self, id: &ComponentId) -> bool {
        id.kind == ComponentKind::Script
    }

    fn list(&self) -> Vec<ComponentId> {
        let mut ids: Vec<ComponentId> = scan_scripts_directory(&self.directory)
            .into_keys()
            .map(|stem| Self::script_id(&stem))
            .collect();
        ids.sort();
        ids
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        if id.kind != ComponentKind::Script {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("ScriptsDirectoryRegistry 只服务 Script 组件：{id}"),
            ));
        }
        let path = self.path_for(&id.name)?;
        let source = std::fs::read_to_string(&path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取脚本失败 {}: {error}", path.display()),
            )
        })?;
        let frontmatter = parse_script_frontmatter(&source);
        let mut frontmatter_map = serde_json::Map::new();
        if let Some(name) = frontmatter.name {
            frontmatter_map.insert("name".to_string(), serde_json::Value::String(name));
        }
        if let Some(description) = frontmatter.description {
            frontmatter_map.insert(
                "description".to_string(),
                serde_json::Value::String(description),
            );
        }
        Ok(ComponentContent {
            frontmatter: Some(serde_json::Value::Object(frontmatter_map)),
            body: source,
        })
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        if id.kind != ComponentKind::Script {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("ScriptsDirectoryRegistry 只服务 Script 组件：{id}"),
            ));
        }
        let path = self.path_for(&id.name)?;
        validate_script(&path, &content.body)
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.validate(id, content)?;
        let path = self.path_for(&id.name)?;
        Self::atomic_write(&path, &content.body)
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        let path = self.path_for(&id.name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("删除脚本失败 {}: {error}", path.display()),
            )),
        }
    }
}
