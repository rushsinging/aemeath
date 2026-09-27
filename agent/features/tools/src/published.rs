//! 窄发布面子命名空间：单一消费方词汇族整体归位（模块即边界）。
//!
//! - [`typed`]：TypedTool 桥接词汇（仅 runtime 消费）
//! - [`sub_run`]：SubRun 生命周期事件（仅 runtime 消费）
//! - [`session_reminder`]：会话提醒注入词汇（仅 runtime 消费）
//! - [`schema_validation`]：工具入参 schema 校验（仅 runtime 消费）

pub mod schema_validation;
pub mod session_reminder;
pub mod sub_run;
pub mod typed;
