/// business/mod.rs — 业务规则（规则专家）：各 Tool 的领域实现
pub mod agent_tool;
pub mod ask_user;
pub mod bash;
pub mod brief;
pub mod config_tool;
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
// 当前仅部分经 tools::api 暴露消费，其余 re-export 保留备用（refs #61 D3）。
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
pub mod sleep;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_list_complete;
pub mod task_list_create;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod tool_search;
pub mod web_fetch;
pub mod web_search;
pub mod worktree;
