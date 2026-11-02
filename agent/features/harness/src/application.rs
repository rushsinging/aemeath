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
pub trait ComponentRegistry: crate::sealed::Sealed + Send {
    /// 枚举全部可编辑组件（稳定排序）。
    fn list(&self) -> Vec<ComponentId>;

    /// 本 registry 是否处理该 kind 的组件（路由新组件用；默认不处理）。
    ///
    /// 组合 registry 路由顺序：已有组件（list 命中）优先于 handles 声明。
    fn handles(&self, id: &ComponentId) -> bool {
        let _ = id;
        false
    }

    /// 读取单个组件的当前内容。
    fn read(&self, id: &ComponentId) -> Result<ComponentContent, HarnessError>;

    /// 校验拟写入内容（不落盘）。校验规则按组件分注册。
    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError>;

    /// 写入组件（实现 MUST 原子写）。
    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<(), HarnessError>;

    /// 删除组件。
    fn delete(&mut self, id: &ComponentId) -> Result<(), HarnessError>;

    /// 读取组件**原始落盘文本**（#2074 回滚用：逐字节恢复，NEVER 经
    /// frontmatter 对象往返重排 key）。默认实现走 content 序列化。
    fn read_raw(&self, id: &ComponentId) -> Result<String, HarnessError> {
        let content = self.read(id)?;
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

    /// 写入**原始文本**（#2074 回滚：先校验解析结果再原子写）。
    /// 默认实现解析为 content 后走 write。
    fn write_raw(&mut self, id: &ComponentId, text: &str) -> Result<(), HarnessError> {
        let content = crate::adapters::filesystem::parse_markdown_with_frontmatter(text)?;
        self.write(id, &content)
    }
}
