use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;
use std::collections::HashMap;

/// dry-run 专用虚拟组件注册表。
///
/// 读取时叠加内存中的虚拟编辑，校验仍委托底层真实 registry；写入/删除只
/// 更新 overlay，NEVER 触碰磁盘。Compose 因此能看到各块编辑叠加后的终态，
/// 同时保持 dry-run 零持久副作用。
pub struct VirtualComponentRegistry<'a> {
    base: &'a mut dyn ComponentRegistry,
    /// `Some(content)` 表示虚拟新增/更新；`None` 表示虚拟删除。
    overlay: HashMap<ComponentId, Option<ComponentContent>>,
}

impl<'a> VirtualComponentRegistry<'a> {
    pub fn new(base: &'a mut dyn ComponentRegistry) -> Self {
        Self {
            base,
            overlay: HashMap::new(),
        }
    }
}

impl ComponentRegistry for VirtualComponentRegistry<'_> {
    fn list(&self) -> Vec<ComponentId> {
        let mut ids: Vec<ComponentId> = self.base.list();
        for (id, content) in &self.overlay {
            match content {
                Some(_) => {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                None => ids.retain(|existing| existing != id),
            }
        }
        ids.sort();
        ids
    }

    fn handles(&self, id: &ComponentId) -> bool {
        self.overlay
            .get(id)
            .map(|content| content.is_some())
            .unwrap_or_else(|| self.base.handles(id))
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        match self.overlay.get(id) {
            Some(Some(content)) => Ok(content.clone()),
            Some(None) => Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("组件不存在：{id}"),
            )),
            None => self.base.read(id),
        }
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.base.validate(id, content)
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.base.validate(id, content)?;
        self.overlay.insert(id.clone(), Some(content.clone()));
        Ok(())
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        if self.read(id).is_err() {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("组件不存在，无法删除：{id}"),
            ));
        }
        self.overlay.insert(id.clone(), None);
        Ok(())
    }

    fn read_raw(&self, id: &ComponentId) -> Result<String, HarnessError> {
        match self.overlay.get(id) {
            Some(Some(content)) => {
                let mut text = String::new();
                if let Some(frontmatter) = &content.frontmatter {
                    let yaml = serde_yml::to_string(frontmatter).map_err(|error| {
                        HarnessError::invalid(
                            crate::constants::ERROR_DOMAIN,
                            format!("frontmatter 序列化失败：{error}"),
                        )
                    })?;
                    text.push_str("---\n");
                    text.push_str(yaml.trim_end());
                    text.push_str("\n---\n");
                }
                text.push_str(&content.body);
                Ok(text)
            }
            Some(None) => Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("组件不存在：{id}"),
            )),
            None => self.base.read_raw(id),
        }
    }

    fn write_raw(&mut self, id: &ComponentId, text: &str) -> Result<(), HarnessError> {
        let content = crate::adapters::filesystem::parse_markdown_with_frontmatter(text)?;
        self.write(id, &content)
    }
}
