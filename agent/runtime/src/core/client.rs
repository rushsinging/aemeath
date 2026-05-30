mod accessors;
mod event;
mod from_args;
mod mapping;
mod trait_accessor;
mod trait_chat;
mod trait_command;
mod trait_impl;
mod trait_session;

pub(crate) use accessors::*;
// re-exports: 对内可见但部分类型仅通过 trait_impl 引用，属正常设计
#[allow(unused_imports)]
pub(crate) use event::*;
#[allow(unused_imports)]
pub(crate) use from_args::*;
#[allow(unused_imports)]
pub(crate) use mapping::*;

// 对外公开导出（CLI / api.rs 通过 runtime::api::client::from_args 访问）
pub use accessors::AgentClientImpl;
pub use from_args::from_args;
