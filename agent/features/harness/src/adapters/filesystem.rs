//! 通用文件组件 adapter：`ComponentId` → 文件路径的注册表实现。
//!
//! - 落盘格式：YAML frontmatter（可选）+ markdown 正文；
//! - `write()` 内建原子写（同目录 tmp + rename），rename 前完成校验；
//! - 具体组件面（skills 目录扫描、overlay 多层链、guardrails 语义校验）
//!   由各组件接入 issue 装配，本 adapter 只提供统一文件读写底座。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 通用文件组件注册表：构造方注入 id → 路径映射。
pub struct FilesystemComponentRegistry {
    component_paths: HashMap<ComponentId, PathBuf>,
}

impl FilesystemComponentRegistry {
    pub fn new(component_paths: HashMap<ComponentId, PathBuf>) -> Self {
        Self { component_paths }
    }

    fn path_of(&self, id: &ComponentId) -> Option<&Path> {
        self.component_paths.get(id).map(|path| path.as_path())
    }

    /// 校验拟写内容：frontmatter 若存在 MUST 是 object。
    fn validate_content_shape(content: &ComponentContent) -> Result<(), HarnessError> {
        match &content.frontmatter {
            Some(value) if !value.is_object() => Err(HarnessError::invalid(
                ERROR_DOMAIN,
                "frontmatter 必须是键值映射（object），拒绝标量或数组",
            )),
            _ => Ok(()),
        }
    }

    /// 序列化为落盘文本（frontmatter 块 + 正文）。
    fn serialize_content(content: &ComponentContent) -> Result<String, HarnessError> {
        let mut text = String::new();
        if let Some(frontmatter) = &content.frontmatter {
            let yaml = serde_yml::to_string(frontmatter).map_err(|error| {
                HarnessError::invalid(ERROR_DOMAIN, format!("frontmatter 序列化失败：{error}"))
            })?;
            text.push_str("---\n");
            text.push_str(yaml.trim_end());
            text.push_str("\n---\n");
        }
        text.push_str(&content.body);
        Ok(text)
    }

    /// 解析落盘文本（可选 frontmatter 块 + 正文）。
    fn parse_content(text: &str) -> Result<ComponentContent, HarnessError> {
        parse_markdown_with_frontmatter(text)
    }
}

/// 解析 markdown + 可选 YAML frontmatter（--- 定界；无定界符时整体为正文）。
/// filesystem 与 skills registry 共用（DRY：组件族统一落盘形态）。
pub fn parse_markdown_with_frontmatter(text: &str) -> Result<ComponentContent, HarnessError> {
    let Some(stripped) = text.strip_prefix("---\n") else {
        // 无 frontmatter 定界符：整个文件都是正文。
        return Ok(ComponentContent {
            frontmatter: None,
            body: text.to_string(),
        });
    };
    let end = stripped
        .find("\n---\n")
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "缺少 frontmatter 结束定界符 ---"))?;
    let frontmatter_text = stripped
        .get(..end)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "frontmatter 区段切分越界"))?;
    let body = stripped
        .get(end + "\n---\n".len()..)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "正文区段切分越界"))?;
    let frontmatter: serde_json::Value =
        serde_yml::from_str(frontmatter_text).map_err(|error| {
            HarnessError::invalid(ERROR_DOMAIN, format!("frontmatter 解析失败：{error}"))
        })?;
    Ok(ComponentContent {
        frontmatter: Some(frontmatter),
        body: body.to_string(),
    })
}

impl FilesystemComponentRegistry {
    /// 原子写：同目录临时文件 + rename（读方 NEVER 看到半写状态）。
    fn atomic_write(path: &Path, text: &str) -> Result<(), HarnessError> {
        let parent_directory = path.parent().ok_or_else(|| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("组件路径缺少父目录：{}", path.display()),
            )
        })?;
        std::fs::create_dir_all(parent_directory).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("创建组件父目录失败 {}: {error}", path.display()),
            )
        })?;
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, text).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("写入临时文件失败 {}: {error}", temp_path.display()),
            )
        })?;
        std::fs::rename(&temp_path, path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("原子替换组件文件失败 {}: {error}", path.display()),
            )
        })?;
        log::debug!(
            target: crate::LOG_TARGET,
            "组件已原子写入：{}",
            path.display()
        );
        Ok(())
    }
}

impl ComponentRegistry for FilesystemComponentRegistry {
    fn list(&self) -> Vec<ComponentId> {
        // list 语义 = 当前存在（文件在盘）的组件；已删除但保留注册的项不列出，
        // 使 ChangeSet Delete 预检的存在性与用户可见清单一致。
        let mut ids: Vec<ComponentId> = self
            .component_paths
            .iter()
            .filter(|(_, path)| path.exists())
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        let path = self.path_of(id).ok_or_else(|| {
            HarnessError::invalid(ERROR_DOMAIN, format!("组件未注册文件路径：{id}"))
        })?;
        let text = std::fs::read_to_string(path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取组件失败 {}: {error}", path.display()),
            )
        })?;
        Self::parse_content(&text)
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        if !self.component_paths.contains_key(id) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("组件未注册文件路径：{id}"),
            ));
        }
        Self::validate_content_shape(content)
    }

    /// 原始文本读（#2074：逐字节，不经 frontmatter 对象往返）。
    fn read_raw(&self, id: &ComponentId) -> Result<String, HarnessError> {
        let path = self.path_of(id).ok_or_else(|| {
            HarnessError::invalid(ERROR_DOMAIN, format!("组件未注册文件路径：{id}"))
        })?;
        std::fs::read_to_string(path).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取组件文件失败 {}: {error}", path.display()),
            )
        })
    }

    /// 原始文本写（#2074：解析校验后原子写，保留原字节布局）。
    fn write_raw(&mut self, id: &ComponentId, text: &str) -> Result<(), HarnessError> {
        let content = parse_markdown_with_frontmatter(text)?;
        self.validate(id, &content)?;
        let path = self.path_of(id).ok_or_else(|| {
            HarnessError::invalid(ERROR_DOMAIN, format!("组件未注册文件路径：{id}"))
        })?;
        Self::atomic_write(path, text)
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.validate(id, content)?;
        let path = self.path_of(id).expect("validate 已确认路径存在");
        let text = Self::serialize_content(content)?;
        Self::atomic_write(path, &text)
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        // 只删文件、保留路径注册：重复 delete（文件已不存在）幂等成功；
        // 后续 Upsert 可在同一注册位重建组件。
        let path = self.path_of(id).ok_or_else(|| {
            HarnessError::invalid(ERROR_DOMAIN, format!("组件未注册文件路径：{id}"))
        })?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("删除组件文件失败 {}: {error}", path.display()),
            )),
        }
    }
}

/// 校验组件文件内容的形状（frontmatter 必须为键值映射）。
///
/// 供热载防御复检（#2015 detect→validate→生效/回退 的 validate 段）：
/// 对监控到的组件文件变更，在生效前复检内容形状，失败则该条变更回退
/// （由调用方保留旧版并通知），NEVER 带病生效。
pub fn validate_component_file_shape(path: &Path) -> Result<(), HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("读取组件文件失败 {}: {error}", path.display()),
        )
    })?;
    let content = FilesystemComponentRegistry::parse_content(&text)?;
    FilesystemComponentRegistry::validate_content_shape(&content)
}
