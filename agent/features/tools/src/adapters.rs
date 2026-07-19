#[cfg(test)]
mod ask_user_tests;
#[cfg(test)]
mod catalog_execution_contract_tests;

/// business/mod.rs — 业务规则（规则专家）：各 Tool 的领域实现
pub mod agent_tool;
pub mod ask_user;
pub mod bash;
pub mod brief;
pub mod catalog;
pub mod composition;
pub mod execution;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob_tool;
pub mod grep;
// 该 Tool 尚未注册到任何 register_* 入口，收窄可见性后内部 API 暂无消费方，
// 保留实现以备后续接线（refs #61 D3）。
#[allow(dead_code)]
pub mod list_mcp_resources;
pub mod lsp;
// mcp / mcp_manager 内含若干面向完整性的辅助类型/函数（diff、sse、validation 等），
// 当前仅部分经 tools crate-root façade 暴露消费，其余 re-export 保留备用（refs #61 D3）。
#[allow(dead_code, unused_imports)]
pub mod mcp;
#[allow(dead_code, unused_imports)]
pub mod mcp_manager;
pub mod mcp_tool; // McpTool is dynamically created, not statically registered
pub mod memory_tool;
pub mod plan_mode;
// 同 list_mcp_resources：尚未注册的 MCP 资源读取 Tool，保留实现（refs #61 D3）。
#[allow(dead_code)]
pub mod read_mcp_resource;
pub mod skill_tool;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_list_complete;
pub mod task_list_create;
pub mod task_stop;
pub mod task_update;
pub mod tool_search;
pub mod web_fetch;
pub mod web_search;
pub mod worktree;

#[cfg(test)]
pub(crate) mod test_support_tests;

/// core/mod.rs — 核心流程（指挥官）：Tool 注册编排
pub mod registry;
pub mod tool_registry;

/// gateway/OHS：工具目录与注册接线
pub mod wiring;

#[cfg(feature = "test-harness")]
pub mod test_harness;
