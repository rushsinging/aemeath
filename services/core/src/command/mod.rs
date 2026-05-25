//! Command system for aemeath
//!
//! Provides a slash command interface with command parsing, registration, and execution.
//!
//! # Architecture
//!
//! - [`CommandRegistry`] — global singleton that holds all commands. Uses `inventory` crate
//!   for compile-time collection; call [`CommandRegistry::initialize`] once at startup.
//! - [`CommandDescriptor`] — value type submitted via `inventory::submit!` in each command module.
//!
//! # Adding a new command
//!
//! 1. Create a file in `commands/` with `inventory::submit! { CommandDescriptor::new(|| Command::new(...)) }`
//! 2. Add `pub mod <name>;` to `commands/mod.rs`
//! 3. Done — the command appears in autocomplete automatically.

pub mod commands;
pub mod parser;
pub mod registry;

pub use commands::{
    Command, CommandAction, CommandCategory, CommandContext, CommandResult, ConfirmAction,
};
pub use parser::{CommandParser, ParseResult};
pub use registry::CommandDescriptor;
pub use registry::CommandRegistry;

/// Builtin command names (without leading slash)
pub mod cmd {
    pub const HELP: &str = "help";
    pub const EXIT: &str = "exit";
    pub const QUIT: &str = "quit";
    pub const CLEAR: &str = "clear";
    pub const COMPACT: &str = "compact";
    pub const RESUME: &str = "resume";
    pub const SESSION: &str = "session";
    pub const REWIND: &str = "rewind";
    pub const CONFIG: &str = "config";
    pub const MODEL: &str = "model";
    pub const PERMISSIONS: &str = "permissions";
    pub const COST: &str = "cost";
    pub const USAGE: &str = "usage";
    pub const STATUS: &str = "status";
    pub const VERSION: &str = "version";
    pub const STATS: &str = "stats";
    pub const TASKS: &str = "tasks";
    pub const MCP: &str = "mcp";
    pub const SKILLS: &str = "skills";
    pub const INIT: &str = "init";
    pub const COMMIT: &str = "commit";
    pub const REVIEW: &str = "review";
    pub const DOCTOR: &str = "doctor";
    pub const THINK: &str = "think";
    pub const MEMORY: &str = "memory";
    pub const REFLECT: &str = "reflect";
}
