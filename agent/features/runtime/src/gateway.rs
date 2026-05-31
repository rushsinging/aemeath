//! Gateway/OHS for constructing and using the runtime implementation.
//!
//! Migration-period exports delegate to the existing client implementation
//! without exposing runtime internals through `runtime::api`.

pub use crate::core::client::{from_args, AgentClientImpl};
