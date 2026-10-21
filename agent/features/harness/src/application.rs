//! ComponentRegistry 端口：8 个可编辑组件的唯一读写面。
//!
//! 实现方（文件系统 adapter、测试桩）MUST 保证：
//! - `list()` 稳定排序（kind 声明序、name 升序）；
//! - `write()` 内建原子写（tmp+rename），rename 前完成 `validate`；
//! - 本端口是组件读写的唯一通道（NEVER 绕过直连文件）。

use crate::domain::component::ComponentContent;
use crate::domain::component::ComponentId;
use crate::domain::errors::HarnessError;

/// 组件树注册表端口。
pub trait ComponentRegistry: Send {
    /// 枚举全部可编辑组件（稳定排序）。
    fn list(&self) -> Vec<ComponentId>;

    /// 读取单个组件的当前内容。
    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError>;

    /// 校验拟写入内容（不落盘）。校验规则按组件分注册。
    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError>;

    /// 写入组件（实现 MUST 原子写）。
    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError>;

    /// 删除组件。
    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError>;
}
