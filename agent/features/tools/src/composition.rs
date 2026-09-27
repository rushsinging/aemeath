//! Composition Root 专用装配面：工具目录/命令/技能的 wire 工厂。

pub use crate::adapters::composition::{
    wire_builtin_catalog_execution, wire_commands, wire_skills, CatalogExecutionWiring,
    CommandWiring, SkillWiring,
};
#[cfg(feature = "test-harness")]
pub use crate::adapters::composition::{TestCatalogExecution, TestCatalogExecutionFactory};
