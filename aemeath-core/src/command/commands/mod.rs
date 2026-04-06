//! Command definitions

pub mod builtin;

use crate::state::AppState;
use crate::config::Config;
use crate::cost::CostTracker;
use std::sync::Arc;

/// Result of executing a command
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command executed successfully with output
    Success(String),
    /// Command executed but needs special handling (e.g., exit)
    Action(CommandAction),
    /// Command failed with error message
    Error(String),
    /// Command requires confirmation
    Confirm {
        message: String,
        action: ConfirmAction,
    },
}

/// Special actions that need to be handled by the caller
#[derive(Debug, Clone)]
pub enum CommandAction {
    /// Exit the application
    Exit,
    /// Clear the screen/history
    Clear,
    /// Compact the message history
    Compact,
    /// Resume a previous session
    ResumeSession(String),
    /// Start a new session
    NewSession,
    /// Change mode
    ChangeMode(String),
}

/// Actions that require confirmation
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    /// Delete a session
    DeleteSession(String),
    /// Clear all history
    ClearAllHistory,
    /// Reset configuration
    ResetConfig,
    /// Clear cost history
    ClearCostHistory,
}

/// Context for command execution
pub struct CommandContext {
    /// Application state
    pub state: Arc<AppState>,
    /// Configuration
    pub config: Config,
    /// Current working directory
    pub cwd: String,
    /// Session ID
    pub session_id: String,
    /// Verbose output
    pub verbose: bool,
    /// Cost tracker
    pub cost_tracker: CostTracker,
}

impl CommandContext {
    /// Create a new command context
    pub fn new(state: Arc<AppState>, config: Config, cwd: String, session_id: String) -> Self {
        let mut cost_tracker = CostTracker::new();
        if let Err(e) = cost_tracker.load() {
            log::warn!("Failed to load cost history: {}", e);
        }
        Self {
            state,
            config,
            cwd,
            session_id,
            verbose: false,
            cost_tracker,
        }
    }

    /// Enable verbose mode
    pub fn verbose(mut self) -> Self {
        self.verbose = true;
        self
    }
}

/// A command definition
pub struct Command {
    /// Command name (without prefix)
    pub name: String,
    /// Short description
    pub description: String,
    /// Usage examples
    pub usage: Vec<String>,
    /// Command aliases
    pub aliases: Vec<String>,
    /// Command category
    pub category: CommandCategory,
    /// Execute function (sync)
    execute_fn: fn(&str, &mut CommandContext) -> CommandResult,
}

/// Command category for grouping
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandCategory {
    /// Core commands (help, exit, clear)
    Core,
    /// Session commands (resume, session)
    Session,
    /// Config commands (config, model)
    Config,
    /// Task commands (tasks)
    Tasks,
    /// Tool commands (mcp, skills)
    Tools,
    /// Git commands (commit, branch)
    Git,
    /// Utility commands (cost, usage, status)
    Utility,
    /// Debug commands (doctor)
    Debug,
}

impl Command {
    /// Create a new command
    pub fn new(
        name: String,
        description: String,
        category: CommandCategory,
        execute: fn(&str, &mut CommandContext) -> CommandResult,
    ) -> Self {
        Self {
            name,
            description,
            usage: Vec::new(),
            aliases: Vec::new(),
            category,
            execute_fn: execute,
        }
    }

    /// Add usage examples
    pub fn with_usage(mut self, usage: Vec<String>) -> Self {
        self.usage = usage;
        self
    }

    /// Add aliases
    pub fn with_aliases(mut self, aliases: Vec<String>) -> Self {
        self.aliases = aliases;
        self
    }

    /// Execute the command
    pub fn execute(&self, args: &str, ctx: &mut CommandContext) -> CommandResult {
        (self.execute_fn)(args, ctx)
    }

    /// Get help text for this command
    pub fn help(&self) -> String {
        let mut help = format!("/{name} - {desc}\n", name = self.name, desc = self.description);

        if !self.usage.is_empty() {
            help.push_str("\nUsage:\n");
            for usage in &self.usage {
                help.push_str(&format!("  {}\n", usage));
            }
        }

        if !self.aliases.is_empty() {
            help.push_str(&format!("\nAliases: {}\n", self.aliases.join(", ")));
        }

        help
    }
}

impl std::fmt::Display for CommandResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandResult::Success(msg) => write!(f, "{}", msg),
            CommandResult::Action(action) => write!(f, "Action: {:?}", action),
            CommandResult::Error(msg) => write!(f, "Error: {}", msg),
            CommandResult::Confirm { message, .. } => write!(f, "Confirm: {}", message),
        }
    }
}