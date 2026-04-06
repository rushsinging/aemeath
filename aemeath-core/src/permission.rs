//! Permission management for tool execution
//!
//! Provides configurable permission control for tool calls.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};

/// Permission modes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Ask for permission on every tool call
    #[default]
    Ask,
    /// Auto-approve read-only tools
    AutoRead,
    /// Auto-approve all tools (dangerous)
    AutoAll,
}

/// Permission decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Allow this tool call
    Allow,
    /// Deny this tool call
    Deny,
    /// Allow this tool for the rest of the session
    AllowAlways,
    /// Allow all tools of this type
    AllowTool,
}

/// Permission manager
pub struct PermissionManager {
    /// Current permission mode
    mode: PermissionMode,
    /// Tools that are always allowed
    allowed_tools: HashSet<String>,
    /// Tools that are always denied
    denied_tools: HashSet<String>,
    /// Tool approvals for this session
    session_approvals: HashSet<String>,
    /// Tool names that are considered read-only
    read_only_tools: HashSet<String>,
    /// Interactive mode (can prompt user)
    interactive: bool,
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionManager {
    /// Create a new permission manager
    pub fn new() -> Self {
        let mut read_only_tools = HashSet::new();
        read_only_tools.insert("Read".to_string());
        read_only_tools.insert("Glob".to_string());
        read_only_tools.insert("Grep".to_string());
        read_only_tools.insert("LSP".to_string());
        read_only_tools.insert("TaskList".to_string());
        read_only_tools.insert("WebFetch".to_string());

        Self {
            mode: PermissionMode::default(),
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            session_approvals: HashSet::new(),
            read_only_tools,
            interactive: true,
        }
    }

    /// Create with specific mode
    pub fn with_mode(mode: PermissionMode) -> Self {
        let mut mgr = Self::new();
        mgr.mode = mode;
        mgr
    }

    /// Set permission mode
    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
    }

    /// Get current mode
    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    /// Set interactive mode
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
    }

    /// Add an allowed tool
    pub fn allow_tool(&mut self, tool_name: &str) {
        self.allowed_tools.insert(tool_name.to_string());
    }

    /// Add a denied tool
    pub fn deny_tool(&mut self, tool_name: &str) {
        self.denied_tools.insert(tool_name.to_string());
    }

    /// Check if a tool is read-only
    pub fn is_read_only(&self, tool_name: &str) -> bool {
        self.read_only_tools.contains(tool_name)
    }

    /// Mark a tool as read-only or not
    pub fn set_read_only(&mut self, tool_name: &str, read_only: bool) {
        if read_only {
            self.read_only_tools.insert(tool_name.to_string());
        } else {
            self.read_only_tools.remove(tool_name);
        }
    }

    /// Check permission for a tool call
    pub fn check_permission(&mut self, tool_name: &str, input: &serde_json::Value) -> PermissionDecision {
        // Check deny list first
        if self.denied_tools.contains(tool_name) {
            return PermissionDecision::Deny;
        }

        // Check allowed list
        if self.allowed_tools.contains(tool_name) {
            return PermissionDecision::Allow;
        }

        // Check session approvals
        if self.session_approvals.contains(tool_name) {
            return PermissionDecision::Allow;
        }

        // Check mode
        match self.mode {
            PermissionMode::AutoAll => PermissionDecision::Allow,
            PermissionMode::AutoRead => {
                if self.is_read_only(tool_name) {
                    PermissionDecision::Allow
                } else {
                    self.prompt_user(tool_name, input)
                }
            }
            PermissionMode::Ask => self.prompt_user(tool_name, input),
        }
    }

    /// Prompt user for permission
    fn prompt_user(&mut self, tool_name: &str, input: &serde_json::Value) -> PermissionDecision {
        if !self.interactive {
            // Non-interactive mode: default to deny unless explicitly allowed
            return PermissionDecision::Deny;
        }

        // Format input for display
        let input_str = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
        let input_preview = if input_str.len() > 500 {
            format!("{}...\n(truncated)", &input_str[..500])
        } else {
            input_str.clone()
        };

        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Tool permission request: {}", tool_name);
        println!("────────────────────────────────────────────────────────────");
        println!("{}", input_preview);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!();
        println!("Options:");
        println!("  [y] Yes (allow once)");
        println!("  [n] No (deny)");
        println!("  [a] Always (allow this tool for rest of session)");
        println!("  [A] All (allow all tools - dangerous!)");
        println!("  [d] Details (show full input)");
        println!("  [q] Quit");

        loop {
            print!("Permission [y/n/a/A/d/q]: ");
            io::stdout().flush().ok();

            let mut input_line = String::new();
            if io::stdin().lock().read_line(&mut input_line).is_err() {
                return PermissionDecision::Deny;
            }

            match input_line.trim().to_lowercase().as_str() {
                "y" | "yes" => return PermissionDecision::Allow,
                "n" | "no" => return PermissionDecision::Deny,
                "a" => {
                    self.session_approvals.insert(tool_name.to_string());
                    return PermissionDecision::AllowTool;
                }
                "all" | "auto" => {
                    self.mode = PermissionMode::AutoAll;
                    return PermissionDecision::AllowAlways;
                }
                "d" | "details" => {
                    println!("\nFull input:");
                    println!("{}", input_str);
                    println!();
                    continue;
                }
                "q" | "quit" => {
                    println!("Exiting...");
                    std::process::exit(0);
                }
                _ => {
                    println!("Invalid option. Use y/n/a/A/d/q");
                    continue;
                }
            }
        }
    }

    /// Check permission without prompting (for batch mode)
    pub fn check_permission_silent(&self, tool_name: &str) -> PermissionDecision {
        if self.denied_tools.contains(tool_name) {
            return PermissionDecision::Deny;
        }

        if self.allowed_tools.contains(tool_name) || self.session_approvals.contains(tool_name) {
            return PermissionDecision::Allow;
        }

        match self.mode {
            PermissionMode::AutoAll => PermissionDecision::Allow,
            PermissionMode::AutoRead => {
                if self.is_read_only(tool_name) {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Deny
                }
            }
            PermissionMode::Ask => PermissionDecision::Deny,
        }
    }
}

/// Tool permission context passed to tools
#[derive(Debug, Clone)]
pub struct PermissionContext {
    /// Current permission mode
    pub mode: PermissionMode,
    /// Whether to auto-approve
    pub auto_approve: bool,
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Ask,
            auto_approve: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_manager_default() {
        let mgr = PermissionManager::new();
        assert_eq!(mgr.mode(), PermissionMode::Ask);
    }

    #[test]
    fn test_auto_all_mode() {
        let mut mgr = PermissionManager::with_mode(PermissionMode::AutoAll);
        let decision = mgr.check_permission_silent("Bash");
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_auto_read_mode() {
        let mut mgr = PermissionManager::with_mode(PermissionMode::AutoRead);
        let read_decision = mgr.check_permission_silent("Read");
        assert_eq!(read_decision, PermissionDecision::Allow);

        let write_decision = mgr.check_permission_silent("Write");
        assert_eq!(write_decision, PermissionDecision::Deny);
    }

    #[test]
    fn test_allow_tool() {
        let mut mgr = PermissionManager::new();
        mgr.allow_tool("Bash");
        let decision = mgr.check_permission_silent("Bash");
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_deny_tool() {
        let mut mgr = PermissionManager::with_mode(PermissionMode::AutoAll);
        mgr.deny_tool("Bash");
        let decision = mgr.check_permission_silent("Bash");
        assert_eq!(decision, PermissionDecision::Deny);
    }
}