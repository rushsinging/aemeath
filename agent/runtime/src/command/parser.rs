//! Command parser - parses user input to extract commands

use std::fmt;

/// Result of parsing user input
#[derive(Debug, Clone)]
pub enum ParseResult {
    /// Not a command (normal input)
    NotCommand(String),
    /// A valid command
    Command { name: String, args: String },
    /// Invalid command format
    Invalid(String),
}

/// Command parser
pub struct CommandParser {
    /// Command prefix (default: '/')
    prefix: char,
}

impl CommandParser {
    /// Create a new parser with default prefix '/'
    pub fn new() -> Self {
        Self { prefix: '/' }
    }

    /// Create a parser with custom prefix
    pub fn with_prefix(prefix: char) -> Self {
        Self { prefix }
    }

    /// Parse user input
    pub fn parse(&self, input: &str) -> ParseResult {
        let trimmed = input.trim();

        // Empty input
        if trimmed.is_empty() {
            return ParseResult::NotCommand(String::new());
        }

        // Check if it starts with the command prefix
        if !trimmed.starts_with(self.prefix) {
            return ParseResult::NotCommand(trimmed.to_string());
        }

        // Extract the command part
        let without_prefix = &trimmed[self.prefix.len_utf8()..];

        // Empty command (just "/")
        if without_prefix.is_empty() {
            return ParseResult::Invalid("Empty command".to_string());
        }

        // Split into name and args
        let parts: Vec<&str> = without_prefix.splitn(2, ' ').collect();
        let name = parts[0].to_lowercase();

        // Validate command name
        if !self.is_valid_name(&name) {
            return ParseResult::Invalid(format!("Invalid command name: {}", name));
        }

        let args = if parts.len() > 1 {
            parts[1].trim().to_string()
        } else {
            String::new()
        };

        ParseResult::Command { name, args }
    }

    /// Check if a command name is valid
    fn is_valid_name(&self, name: &str) -> bool {
        // Command names must be alphanumeric with optional hyphens/underscores
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !name.is_empty()
    }

    /// Get suggestions for autocomplete
    pub fn suggestions(&self, input: &str, commands: &[&str]) -> Vec<String> {
        let trimmed = input.trim();

        // Only suggest if it starts with prefix
        if !trimmed.starts_with(self.prefix) {
            return Vec::new();
        }

        let without_prefix = &trimmed[self.prefix.len_utf8()..];
        let partial = without_prefix.split_whitespace().next().unwrap_or("");

        commands
            .iter()
            .filter(|cmd| cmd.starts_with(partial))
            .map(|cmd| format!("{}{}", self.prefix, cmd))
            .collect()
    }
}

impl Default for CommandParser {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ParseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseResult::NotCommand(text) => write!(f, "Input: {}", text),
            ParseResult::Command { name, args } => {
                if args.is_empty() {
                    write!(f, "Command: /{}", name)
                } else {
                    write!(f, "Command: /{} {}", name, args)
                }
            }
            ParseResult::Invalid(reason) => write!(f, "Invalid: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_not_command() {
        let parser = CommandParser::new();
        let result = parser.parse("hello world");
        assert!(matches!(result, ParseResult::NotCommand(_)));
    }

    #[test]
    fn test_parse_simple_command() {
        let parser = CommandParser::new();
        let result = parser.parse("/help");
        match result {
            ParseResult::Command { name, args } => {
                assert_eq!(name, "help");
                assert_eq!(args, "");
            }
            _ => panic!("Expected Command"),
        }
    }

    #[test]
    fn test_parse_command_with_args() {
        let parser = CommandParser::new();
        let result = parser.parse("/config set api.key mykey");
        match result {
            ParseResult::Command { name, args } => {
                assert_eq!(name, "config");
                assert_eq!(args, "set api.key mykey");
            }
            _ => panic!("Expected Command"),
        }
    }

    #[test]
    fn test_parse_empty_command() {
        let parser = CommandParser::new();
        let result = parser.parse("/");
        assert!(matches!(result, ParseResult::Invalid(_)));
    }

    #[test]
    fn test_parse_invalid_name() {
        let parser = CommandParser::new();
        let result = parser.parse("/hello!");
        assert!(matches!(result, ParseResult::Invalid(_)));
    }

    #[test]
    fn test_suggestions() {
        let parser = CommandParser::new();
        let commands = vec!["help", "hello", "history", "exit"];
        let suggestions = parser.suggestions("/h", &commands);
        assert_eq!(suggestions.len(), 3);
    }
}
