/// business/mod.rs — 业务规则（规则专家）：持久化领域（memory / history / 超长工具结果落盘）
// history 模块当前无任何消费方（TUI 输入历史由 apps/cli 自有实现承载），
// 收窄可见性后内部 API 暴露为死代码，保留实现以备后续接线（refs #61 D3）。
#[allow(dead_code)]
pub mod history;
pub mod memory;
pub mod tool_result_storage;
