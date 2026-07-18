//! Tool 执行所需的共享资源（tools crate 自包含）。
//!
//! 由 runtime 的 `RuntimeResources` 构造时映射填充。
//! 详见 `docs/design/03-runtime-design.md` §"Runtime Context 职责边界"。

use std::sync::Arc;

use share::config::MemoryConfig;

use super::{AgentRunner, MemoryPortSource, ToolListProvider};

/// Tool 执行所需的共享资源——跨 session 不变的配置和服务句柄。
///
/// 在 `tools` crate 内定义（而非直接引用 `runtime::RuntimeResources`），
/// 因为 `tools` 不依赖 `runtime`，且 tool 执行只需要这些字段的子集。
#[derive(Clone)]
pub struct ToolResources {
    /// Sub-agent runner（Agent 工具调用时使用）。
    pub agent_runner: Option<Arc<dyn AgentRunner>>,
    /// Tool list provider（ToolSearch 动态查询用）。
    /// 主 chat loop 设置；子 agent context 为 `None`。
    pub registry: Option<Arc<dyn ToolListProvider>>,
    /// Memory system configuration（MemoryTool 使用）。
    pub memory_config: MemoryConfig,
    /// Memory port source（MemoryTool 使用）。
    /// Runtime/Composition 提供，从 `MainSessionWiring::committed_memory()` 取值。
    /// 子 agent 注册 fresh registry 时从父 context 克隆，保持同一项目绑定。
    pub memory_source: Arc<dyn MemoryPortSource>,
    /// Current language code (`"en"` / `"zh"`)，用于选择 i18n 文案。
    pub lang: String,
    /// Whether all tools are auto-approved（跳过权限检查）。
    pub allow_all: bool,
}
