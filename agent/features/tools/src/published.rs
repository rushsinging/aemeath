//! 窄发布面子命名空间：单一消费方词汇族整体归位（模块即边界）。
//!
//! - [`typed`]：TypedTool 桥接词汇（仅 runtime 消费）
//! - [`sub_run`]：SubRun 生命周期事件（仅 runtime 消费）
//! - [`schema_validation`]：工具入参 schema 校验（仅 runtime 消费）
//! - [`snapshot_query`]：快照查询命令词汇（仅 sdk 消费）
//! - [`skill`]：技能目录/加载状态机词汇（多 crate）
//! - [`command`]：slash 命令发布语言（sdk 二次发布）
//! - [`agent`]：子代理派发词汇（runtime/composition）
//! - [`execution`]：工具执行生命周期词汇（含取消/确认/进度）

pub mod agent;
pub mod command;
pub mod execution;
pub mod schema_validation;
pub mod skill;
pub mod snapshot_query;
pub mod sub_run;
pub mod typed;
