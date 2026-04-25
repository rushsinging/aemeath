//! Command system for aemeath
//!
//! Provides a slash command interface similar to extracted_sources,
//! with command parsing, registration, and execution.

pub mod registry;
pub mod parser;
pub mod commands;

pub use registry::CommandRegistry;
pub use parser::{CommandParser, ParseResult};
pub use commands::{Command, CommandContext, CommandResult, CommandAction, ConfirmAction, CommandCategory};

/// Builtin command names (without leading slash)
pub mod cmd {
    /// Core commands
    pub const HELP: &str = "help";
    pub const EXIT: &str = "exit";
    pub const QUIT: &str = "quit";
    pub const CLEAR: &str = "clear";
    pub const COMPACT: &str = "compact";
    
    /// Session commands
    pub const RESUME: &str = "resume";
    pub const SESSION: &str = "session";
    pub const REWIND: &str = "rewind";
    
    /// Config commands
    pub const CONFIG: &str = "config";
    pub const MODEL: &str = "model";
    pub const PERMISSIONS: &str = "permissions";
    
    /// Utility commands
    pub const COST: &str = "cost";
    pub const USAGE: &str = "usage";
    pub const STATUS: &str = "status";
    pub const VERSION: &str = "version";
    pub const STATS: &str = "stats";
    
    /// Tool commands
    pub const TASKS: &str = "tasks";
    pub const TODO: &str = "todo";
    pub const MCP: &str = "mcp";
    pub const SKILLS: &str = "skills";
    
    /// Git commands
    pub const INIT: &str = "init";
    pub const COMMIT: &str = "commit";
    pub const REVIEW: &str = "review";
    
    /// Debug commands
    pub const DOCTOR: &str = "doctor";
}