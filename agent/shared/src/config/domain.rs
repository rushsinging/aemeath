//! Config domain layer — DDD aggregate root, value objects, merge strategy.
//!
//! Pure domain logic. NEVER touches fs / env / network.

pub mod audit;
pub mod config;
pub mod driver_env;
pub mod file_snapshot;
pub mod hooks;
pub mod legacy;
pub mod logging;
pub mod memory;
pub mod merge;
pub mod models;
pub mod permissions;
pub mod scope;
pub mod skills;
pub mod snapshot;
pub mod storage;
pub mod tools;
pub mod ui;
pub mod update;
