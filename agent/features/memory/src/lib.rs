#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Memory 支撑域。

mod adapters;
mod codec;
mod domain;
mod ports;
mod service;

pub use adapters::{
    map_storage_error, AtomicDatasetMemoryStore, InMemoryMemory, MemoryPolicy, ProjectMemoryOpener,
};
pub use domain::*;
pub use ports::*;
pub use service::MemoryService;
