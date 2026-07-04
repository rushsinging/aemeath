//! Config domain layer — DDD aggregate root, value objects, merge strategy.
//!
//! Pure domain logic. NEVER touches fs / env / network.

pub mod config;
pub mod driver_env;
pub mod merge;
pub mod snapshot;
