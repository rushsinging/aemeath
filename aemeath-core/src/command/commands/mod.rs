//! Command definitions
//!
//! Each module uses `inventory::submit!` to declare commands at compile time.
//! Adding a new command only requires:
//!
//! 1. Create the file with `inventory::submit! { CommandDescriptor::new(|| Command::new(...)) }`
//! 2. Add `pub mod <name>;` to this file
//!
//! The command automatically appears in TUI autocomplete.

pub mod help;
pub mod misc;
pub mod session;
pub mod model;
pub mod config_cmd;
pub mod tasks;
pub mod tools;
pub mod git;
pub mod debug;
pub mod stats;
pub mod think;
pub mod effort;

/// Initialize all built-in commands.
///
/// Delegates to [`CommandRegistry::initialize`] which iterates all
/// `inventory::submit!`ed command descriptors.
///
/// No manual per-command registration — just add the module and it's automatic.
pub fn init_all() {
    crate::command::CommandRegistry::initialize();
}

use crate::state::AppState;
use crate::config::Config;
use crate::cost::CostTracker;
use std::future::Future;
use std::pin::Pin;
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
    /// Switch to a different model
    SwitchModel {
        provider_name: String,
        model_id: String,
        /// Display name for UI (falls back to model_id if empty)
        model_name: String,
        base_url: String,
        api_key: String,
        api_type: String,
        max_tokens: u32,
        context_window: usize,
        reasoning: Option<bool>,
    },
    /// Inject a user message into the conversation (e.g. review, skill, commit)
    InjectMessage(String),
    /// Run a skill — injects skill content as a user message
    RunSkill(String),
    /// Toggle reasoning/thinking mode (None = toggle)
    SetThinking(Option<bool>),
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
    /// Multi-model configuration
    pub models_config: crate::config::ModelsConfig,
    /// Current model name (for display)
    pub current_model: String,
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
            models_config: crate::config::ModelsConfig::default(),
            current_model: String::new(),
        }
    }

    /// Enable verbose mode
    pub fn verbose(mut self) -> Self {
        self.verbose = true;
        self
    }
}

/// Type alias for the async execute function stored in Command.
type AsyncExecuteFn = Box<dyn Fn(&str, &mut CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> + Send + Sync>;

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
    execute_fn: AsyncExecuteFn,
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
    /// Create a new command from a sync execute function
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
            execute_fn: Box::new(move |args: &str, ctx: &mut CommandContext| {
                let result = execute(args, ctx);
                Box::pin(async move { result })
            }),
        }
    }

    /// Create a new command from an async execute function.
    /// The closure receives owned `String` args and `&mut CommandContext`.
    pub fn new_async(
        name: String,
        description: String,
        category: CommandCategory,
        execute: impl Fn(String, &mut CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            description,
            usage: Vec::new(),
            aliases: Vec::new(),
            category,
            execute_fn: Box::new(move |args: &str, ctx: &mut CommandContext| {
                execute(args.to_string(), ctx)
            }),
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

    /// Execute the command (async)
    pub async fn execute(&self, args: &str, ctx: &mut CommandContext) -> CommandResult {
        (self.execute_fn)(args, ctx).await
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
