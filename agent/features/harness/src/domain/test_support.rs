//! 领域测试共享支撑：内存版 registry 桩，供契约与语义测试使用。

use crate::application::ComponentRegistry;
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;
use std::collections::HashMap;

/// 内存 registry 桩：记录 validate 调用，可注入指定 id 的校验失败。
pub struct FakeRegistry {
    entries: HashMap<ComponentId, ComponentContent>,
    /// 命中这些 id 时 validate 返回失败（模拟非法内容）。
    pub invalid_ids: Vec<ComponentId>,
    /// write/delete 是否发生过（断言「整份作废」时无写入）。
    pub writes: Vec<ComponentId>,
    pub deletes: Vec<ComponentId>,
}

impl FakeRegistry {
    pub fn with_entries(entries: Vec<(ComponentId, ComponentContent)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
            invalid_ids: Vec::new(),
            writes: Vec::new(),
            deletes: Vec::new(),
        }
    }

    pub fn content_of(&self, id: &ComponentId) -> Option<&ComponentContent> {
        self.entries.get(id)
    }
}

impl ComponentRegistry for FakeRegistry {
    fn list(&self) -> Vec<ComponentId> {
        let mut ids: Vec<ComponentId> = self.entries.keys().cloned().collect();
        ids.sort();
        ids
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        self.entries.get(id).cloned().ok_or_else(|| {
            HarnessError::invalid(crate::constants::ERROR_DOMAIN, format!("组件不存在：{id}"))
        })
    }

    fn validate(&self, id: &ComponentId, _content: &ComponentContent) -> Result<(), HarnessError> {
        if self.invalid_ids.contains(id) {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("组件校验失败：{id}"),
            ));
        }
        Ok(())
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.writes.push(id.clone());
        self.entries.insert(id.clone(), content.clone());
        Ok(())
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        self.deletes.push(id.clone());
        self.entries.remove(id);
        Ok(())
    }
}

// #2146 密封解锁：实现权归属声明（Sealed 于定义 crate 内授予）。
impl crate::sealed::Sealed for FakeRegistry {}
