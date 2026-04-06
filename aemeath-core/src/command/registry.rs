//! Command registry - manages all available commands

use crate::command::{Command, CommandContext, CommandResult};
use std::collections::HashMap;

/// Command registry that holds all registered commands
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
}

impl CommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Create a registry with default commands
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }

    /// Register a command
    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Register all default commands
    fn register_defaults(&mut self) {
        use crate::command::commands::builtin::*;

        self.register(help_command());
        self.register(exit_command());
        self.register(clear_command());
        self.register(compact_command());
        self.register(cost_command());
        self.register(usage_command());
        self.register(status_command());
        self.register(config_command());
        self.register(resume_command());
        self.register(session_command());
        self.register(version_command());
        self.register(model_command());
        self.register(tasks_command());
        self.register(mcp_command());
        self.register(skills_command());
        self.register(permissions_command());
        self.register(doctor_command());
        self.register(init_command());
        self.register(commit_command());
        self.register(rewind_command());
        self.register(review_command());
        self.register(stats_command());
    }

    /// Get a command by name (checks main name and aliases)
    pub fn find(&self, name: &str) -> Option<&Command> {
        // Check main name first
        if let Some(cmd) = self.commands.get(name) {
            return Some(cmd);
        }
        // Then check aliases (without leading slash)
        let name = name.trim_start_matches('/');
        self.commands.values().find(|cmd| cmd.aliases.contains(&name.to_string()))
    }

    /// Get a command by main name only
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands.get(name)
    }

    /// Get command name (with slash prefix)
    pub fn command_name(&self, name: &str) -> String {
        if let Some(cmd) = self.find(name) {
            format!("/{}", cmd.name)
        } else if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{}", name)
        }
    }

    /// List all commands
    pub fn list(&self) -> Vec<&Command> {
        let mut commands: Vec<_> = self.commands.values().collect();
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands
    }

    /// Execute a command
    pub fn execute(&self, name: &str, args: &str, ctx: &mut CommandContext) -> CommandResult {
        if let Some(command) = self.commands.get(name) {
            command.execute(args, ctx)
        } else {
            CommandResult::Error(format!("Unknown command: /{}", name))
        }
    }

    /// Get command suggestions for autocomplete (checks main names and aliases)
    pub fn suggestions(&self, prefix: &str) -> Vec<&Command> {
        let prefix = prefix.trim_start_matches('/');
        self.commands
            .values()
            .filter(|cmd| {
                cmd.name.starts_with(prefix) || 
                cmd.aliases.iter().any(|a| a.starts_with(prefix))
            })
            .collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}