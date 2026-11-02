//! 组合 registry：多个 registry 合并为一个 ComponentRegistry 视图，
//! list 合并、read/validate/write/delete 按 id 路由（c5/c7 多实例接入）。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;

/// 组合注册表：首个能处理该 id 的成员胜出（成员间职责不重叠）。
pub struct CompositeComponentRegistry {
    members: Vec<Box<dyn ComponentRegistry>>,
}

impl CompositeComponentRegistry {
    pub fn new(members: Vec<Box<dyn ComponentRegistry>>) -> Self {
        Self { members }
    }

    fn route(&self, id: &ComponentId) -> Result<&dyn ComponentRegistry, HarnessError> {
        self.members
            .iter()
            .find(|member| member.list().contains(id) || member.handles(id))
            .map(|member| member.as_ref())
            .ok_or_else(|| {
                HarnessError::invalid(ERROR_DOMAIN, format!("无 registry 成员可路由组件：{id}"))
            })
    }
}

impl ComponentRegistry for CompositeComponentRegistry {
    fn list(&self) -> Vec<ComponentId> {
        let mut ids: Vec<ComponentId> = self
            .members
            .iter()
            .flat_map(|member| member.list())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    fn handles(&self, id: &ComponentId) -> bool {
        self.members.iter().any(|member| member.handles(id))
    }

    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError> {
        self.route(id)?.read(id)
    }

    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        self.route(id)?.validate(id, content)
    }

    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.list().contains(id) || member.handles(id))
            .ok_or_else(|| {
                HarnessError::invalid(ERROR_DOMAIN, format!("无 registry 成员可路由组件：{id}"))
            })?;
        member.write(id, content)
    }

    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.list().contains(id) || member.handles(id))
            .ok_or_else(|| {
                HarnessError::invalid(ERROR_DOMAIN, format!("无 registry 成员可路由组件：{id}"))
            })?;
        member.delete(id)
    }
}

// #2146 密封解锁：实现权归属声明（Sealed 于定义 crate 内授予）。
impl crate::sealed::Sealed for CompositeComponentRegistry {}
