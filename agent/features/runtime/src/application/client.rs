mod accessors;
mod from_args;
mod mapping;
mod trait_chat;
mod trait_impl;
mod trait_memory;
mod trait_model;
mod trait_reflection;
mod trait_session;

pub(crate) use accessors::*;
#[allow(unused_imports)]
pub(crate) use from_args::*;
#[allow(unused_imports)]
pub(crate) use mapping::*;

// 对外公开导出（gateway 通过 runtime::{from_args, AgentClientImpl} 公开）
pub use accessors::AgentClientImpl;
pub use from_args::{from_args, from_args_with_workspace};
