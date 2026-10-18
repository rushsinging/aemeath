//! background_process — 后台进程监督（tool call 统一后台进程模型）。
//!
//! 对应设计：`docs/design/02-modules/runtime/09-background-tasks.md`。
//!
//! 监督器是 session 级状态容器：所有 tool call 派发即登记，超阈值转后台，
//! Run 收口不销毁；非终态记录在 resume 恢复时统一失效（restore_records）。
//! 逐 call 执行事实（receipt / 取消协议）仍由 Context 承担；完成通知
//! （reminder / Wakeup Run）挂接点在通知链路交付中落地。

pub(crate) mod activity_log;
pub(crate) mod log_file;
pub(crate) mod session_runtime;
pub(crate) mod supervisor;
