pub mod api;
// history 模块当前无任何消费方（TUI 输入历史由 apps/cli 自有实现承载），
// 收窄可见性后内部 API 暴露为死代码，保留实现以备后续接线（refs #61 D3）。
#[allow(dead_code)]
mod history;
mod memory;
mod tool_result_storage;
