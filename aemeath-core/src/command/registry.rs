//! Command registry — storage, query, and global singleton for commands.
//!
//! Commands are collected at compile time via `inventory::submit!` in each
//! command module. Call [`CommandRegistry::initialize`] once at startup to
//! populate the global registry.

use crate::command::{Command, CommandContext, CommandResult};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// CommandDescriptor — the value type collected by `inventory`
// ---------------------------------------------------------------------------

/// A command descriptor submitted via `inventory::submit!`.
///
/// Wraps a factory function `fn() -> Command` so that `inventory` can collect
/// it at link time without requiring `Command` to be `Sync`.
pub struct CommandDescriptor {
    factory: fn() -> Command,
}

impl CommandDescriptor {
    /// Create a new descriptor from a zero-arg factory function.
    pub const fn new(factory: fn() -> Command) -> Self {
        Self { factory }
    }

    /// Build the command from this descriptor.
    pub fn build(&self) -> Command {
        (self.factory)()
    }
}

// The magic: `inventory` collects all `submit!` calls into an iterator.
inventory::collect!(CommandDescriptor);

// ---------------------------------------------------------------------------
// CommandRegistry — storage + global singleton
// ---------------------------------------------------------------------------

/// Command registry that holds all registered commands.
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
}

impl CommandRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Initialize the global registry by iterating all `inventory::submit!`ed
    /// command descriptors. Must be called once at application startup.
    pub fn initialize() {
        let mut registry = Self::global();
        for descriptor in inventory::iter::<CommandDescriptor> {
            let cmd = descriptor.build();
            registry.register(cmd);
        }
    }

    /// Access the global registry.
    pub fn global() -> std::sync::MutexGuard<'static, Self> {
        static INSTANCE: std::sync::LazyLock<std::sync::Mutex<CommandRegistry>> =
            std::sync::LazyLock::new(|| std::sync::Mutex::new(CommandRegistry::new()));
        INSTANCE.lock().expect("CommandRegistry lock poisoned")
    }

    /// Register a command.
    fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Get a command by name (checks main name and aliases).
    pub fn find(&self, name: &str) -> Option<&Command> {
        if let Some(cmd) = self.commands.get(name) {
            return Some(cmd);
        }
        let name = name.trim_start_matches('/');
        self.commands.values().find(|cmd| cmd.aliases.contains(&name.to_string()))
    }

    /// Get a command by main name only.
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands.get(name)
    }

    /// List all commands sorted by name.
    pub fn list(&self) -> Vec<&Command> {
        let mut commands: Vec<_> = self.commands.values().collect();
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands
    }

    /// Execute a command.
    pub async fn execute(&self, name: &str, args: &str, ctx: &mut CommandContext) -> CommandResult {
        if let Some(command) = self.find(name) {
            command.execute(args, ctx).await
        } else {
            CommandResult::Error(format!("Unknown command: /{}", name.trim_start_matches('/')))
        }
    }

    /// Get command suggestions for autocomplete (checks main names and aliases).
    pub fn suggestions(&self, prefix: &str) -> Vec<&Command> {
        let prefix = prefix.trim_start_matches('/');
        self.commands
            .values()
            .filter(|cmd| {
                cmd.name.starts_with(prefix)
                    || cmd.aliases.iter().any(|a| a.starts_with(prefix))
            })
            .collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
