#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Memory 支撑域。

mod adapters;
mod domain;
mod ports;

pub use adapters::{InMemoryMemory, MemoryPolicy};
pub use domain::*;
pub use ports::*;
