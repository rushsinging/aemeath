//! Autocomplete module for handling / and @ trigger completions

use std::path::PathBuf;

/// A single suggestion item
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Unique identifier
    pub _id: String,
    /// Text to display
    pub display_text: String,
    /// Optional description
    pub _description: Option<String>,
    /// Suggestion type
    pub suggestion_type: SuggestionType,
}

/// Type of suggestion
#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionType {
    Command,
    File,
    Directory,
    Model,
}

/// Context for generating suggestions
#[derive(Debug)]
pub struct SuggestionContext {
    /// The full input text
    pub input: String,
    /// Cursor position in the input
    pub cursor_offset: usize,
    /// Current working directory
    pub cwd: PathBuf,
    /// Available models for /model completion: list of (provider_name, model_id)
    pub models: Vec<(String, String)>,
    /// Available skills: list of (name, description, aliases)
    pub skills: Vec<(String, String, Vec<String>)>,
}

/// Slash command definition
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
}

/// Get all available slash commands
pub fn get_slash_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            name: "help".to_string(),
            description: "Show available commands".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "exit".to_string(),
            description: "Exit the agent".to_string(),
            aliases: vec!["quit".to_string()],
        },
        SlashCommand {
            name: "clear".to_string(),
            description: "Clear conversation history".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "compact".to_string(),
            description: "Manually compact conversation".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "usage".to_string(),
            description: "Show token usage statistics".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "context".to_string(),
            description: "Show context window usage".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "save".to_string(),
            description: "Save current session to disk".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "sessions".to_string(),
            description: "List saved sessions".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "commit".to_string(),
            description: "Stage changes and create git commit".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "image".to_string(),
            description: "Add an image to next message".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "paste".to_string(),
            description: "Read image from clipboard".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "images".to_string(),
            description: "Show pending images".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "clear-images".to_string(),
            description: "Clear pending images".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "review".to_string(),
            description: "Review code changes (git diff)".to_string(),
            aliases: vec!["rev".to_string()],
        },
        // Model-related commands
        SlashCommand {
            name: "model".to_string(),
            description: "Show/switch model (use /model list to see available)".to_string(),
            aliases: vec![],
        },
    ]
}

/// Extract the completion token at cursor position
/// Returns (token, start_position, trigger_type) if a trigger is found
pub fn extract_completion_token(input: &str, cursor_offset: usize) -> Option<(String, usize, TriggerType)> {
    if input.is_empty() || cursor_offset == 0 {
        return None;
    }

    // Ensure cursor_offset is at a valid char boundary
    let cursor_offset = if cursor_offset >= input.len() {
        input.len()
    } else if input.is_char_boundary(cursor_offset) {
        cursor_offset
    } else {
        // Find the nearest valid char boundary before cursor_offset
        let mut pos = cursor_offset;
        while pos > 0 && !input.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    };

    if cursor_offset == 0 {
        return None;
    }

    let before_cursor = &input[..cursor_offset];

    // Check for /model <arg> trigger (model name completion)
    if input.starts_with("/model ") && cursor_offset >= 7 {
        let arg_start = 7; // length of "/model "
        let after_cmd = &input[arg_start..];
        // Don't trigger for "/model list" or "/model list ..."
        if after_cmd.starts_with("list") && (after_cmd.len() == 4 || after_cmd.as_bytes()[4] == b' ') {
            // fall through to other triggers
        } else {
            let arg_part = &input[arg_start..cursor_offset];
            // Include text after cursor until whitespace for matching
            let after_cursor = &input[cursor_offset..];
            let after_until_space: String = after_cursor
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            let full_arg = format!("{}{}", arg_part, after_until_space);
            return Some((full_arg, arg_start, TriggerType::ModelArg));
        }
    }

    // Check for /model subcommand completion (e.g., /model l -> /model list)
    if input.starts_with("/model") && cursor_offset > 6 {
        // Check if there's no space yet (partial command)
        if input.len() > 6 && !input.chars().nth(6).map_or(false, |c| c.is_whitespace()) {
            // Still typing "/model..." command
        } else {
            // "/model " with potential subcommand
            let _after_model = if input.len() > 6 {
                &input[6..]  // skip "/model"
            } else {
                ""
            };
            // Return as ModelSubCommand if it looks like a subcommand (not a full model name)
            let arg_part = &input[7..cursor_offset].trim_start(); // skip "/model "
            if !arg_part.is_empty() {
                return Some((arg_part.to_string(), 7, TriggerType::ModelSubCommand));
            }
        }
    }

    // Check for @ trigger (file/path completion)
    if let Some(at_pos) = before_cursor.rfind('@') {
        let is_start_or_after_space = at_pos == 0
            || before_cursor[..at_pos].ends_with(char::is_whitespace);
        if is_start_or_after_space {
            let after_at = &before_cursor[at_pos + 1..]; // '@' is ASCII, +1 is safe
            let after_cursor = &input[cursor_offset..];
            // Get text after cursor until whitespace (space, newline, or tab)
            let after_cursor_until_space = after_cursor
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect::<String>();
            let full_token = format!("@{}{}", after_at, after_cursor_until_space);
            return Some((full_token, at_pos, TriggerType::AtSymbol));
        }
    }

    // Check for / trigger (slash command completion)
    if input.starts_with('/') {
        let end = input.find(' ').unwrap_or(input.len()).min(cursor_offset);
        let token = &before_cursor[..end];
        return Some((token.to_string(), 0, TriggerType::SlashCommand));
    }

    // Check for mid-input slash command (whitespace followed by /)
    if let Some(space_slash_pos) = before_cursor.rfind(" /") {
        let slash_pos = space_slash_pos + 1; // ' ' is ASCII, +1 is safe
        let after_slash = &before_cursor[slash_pos + 1..]; // '/' is ASCII, +1 is safe
        if !after_slash.contains(' ') {
            let token = format!("/{}", after_slash);
            return Some((token, slash_pos, TriggerType::SlashCommand));
        }
    }

    None
}

/// Trigger type for completion
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerType {
    SlashCommand,
    AtSymbol,
    ModelArg,
    ModelSubCommand,
}

/// Generate slash command suggestions based on partial input.
/// Also includes skill names/aliases as suggestions.
pub fn generate_command_suggestions(partial: &str, skills: &[(String, String, Vec<String>)]) -> Vec<Suggestion> {
    let commands = get_slash_commands();
    let partial_lower = partial.to_lowercase();
    
    // Remove leading / if present
    let search_term = if partial_lower.starts_with('/') {
        &partial_lower[1..]
    } else {
        &partial_lower
    };

    let mut results = Vec::new();

    if search_term.is_empty() {
        // Return all commands + all skills
        for cmd in &commands {
            results.push(Suggestion {
                _id: format!("cmd-{}", cmd.name),
                display_text: format!("/{}", cmd.name),
                _description: Some(cmd.description.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
        for (name, desc, _aliases) in skills {
            results.push(Suggestion {
                _id: format!("skill-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
        return results;
    }

    // Filter commands by partial match on name or aliases
    for cmd in &commands {
        if cmd.name.starts_with(search_term) ||
            cmd.aliases.iter().any(|a| a.starts_with(search_term))
        {
            results.push(Suggestion {
                _id: format!("cmd-{}", cmd.name),
                display_text: format!("/{}", cmd.name),
                _description: Some(cmd.description.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
    }

    // Filter skills by partial match on name or aliases
    for (name, desc, aliases) in skills {
        if name.starts_with(search_term) ||
            aliases.iter().any(|a| a.starts_with(search_term))
        {
            results.push(Suggestion {
                _id: format!("skill-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
    }

    results
}

/// Generate model suggestions based on partial input for /model command
pub fn generate_model_suggestions(partial: &str, models: &[(String, String)]) -> Vec<Suggestion> {
    if models.is_empty() {
        return Vec::new();
    }

    let partial_lower = partial.to_lowercase();

    models
        .iter()
        .filter(|(provider, model_id)| {
            let full = format!("{}/{}", provider, model_id);
            full.to_lowercase().starts_with(&partial_lower)
                || provider.to_lowercase().starts_with(&partial_lower)
        })
        .map(|(provider, model_id)| {
            Suggestion {
                _id: format!("model-{}/{}", provider, model_id),
                display_text: format!("{}/{}", provider, model_id),
                _description: None,
                suggestion_type: SuggestionType::Model,
            }
        })
        .collect()
}

/// Generate model subcommand suggestions for /model command
pub fn generate_model_subcommand_suggestions(partial: &str) -> Vec<Suggestion> {
  let subcommands = vec![
      ("list", "List available models from config"),
  ];
    
  let partial_lower = partial.to_lowercase();
    
  subcommands
      .iter()
      .filter(|(name, _desc)| name.to_lowercase().starts_with(&partial_lower))
      .map(|(name, desc)| Suggestion {
          _id: format!("model-subcmd-{}", name),
          display_text: format!("list"),
          _description: Some(desc.to_string()),
          suggestion_type: SuggestionType::Command,
      })
      .collect()
}

/// Generate file/directory suggestions based on partial path
pub fn generate_file_suggestions(partial: &str, cwd: &PathBuf) -> Vec<Suggestion> {
    // Remove @ prefix if present
    let path_str = if partial.starts_with('@') {
        &partial[1..]
    } else {
        partial
    };

    if path_str.is_empty() {
        // Return current directory contents
        return list_directory_contents(cwd);
    }

    // Parse the path
    let path = if path_str.starts_with('/') {
        PathBuf::from(path_str)
    } else if path_str.starts_with('~') {
        // Expand home directory
        if let Some(home) = std::env::var("HOME").ok() {
            PathBuf::from(home).join(&path_str[1..])
        } else {
            cwd.join(path_str)
        }
    } else if path_str.starts_with('.') {
        cwd.join(path_str)
    } else {
        cwd.join(path_str)
    };

    // Get the directory to list and the prefix to filter
    let (dir_to_list, filter_prefix) = if path.is_dir() {
        (path, "".to_string())
    } else {
        let parent = path.parent();
        let filename = path.file_name();
        match (parent, filename) {
            (Some(p), Some(f)) => (p.to_path_buf(), f.to_string_lossy().to_string()),
            (None, Some(f)) => (cwd.clone(), f.to_string_lossy().to_string()),
            _ => (cwd.clone(), "".to_string()),
        }
    };

    list_and_filter_directory(&dir_to_list, &filter_prefix, cwd)
}

/// List contents of a directory
fn list_directory_contents(dir: &PathBuf) -> Vec<Suggestion> {
    list_and_filter_directory(dir, "", dir)
}

/// List directory contents and filter by prefix
fn list_and_filter_directory(dir: &PathBuf, prefix: &str, base_dir: &PathBuf) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return suggestions;
    }

    let entries = std::fs::read_dir(dir);
    if entries.is_err() {
        return suggestions;
    }

    let prefix_lower = prefix.to_lowercase();
    
    if let Ok(entries) = entries {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Filter by prefix
        if !prefix_lower.is_empty() && !name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        // Skip hidden files (unless prefix starts with .)
        if name.starts_with('.') && !prefix_lower.starts_with('.') {
            continue;
        }

        let is_dir = path.is_dir();
        
        // Calculate relative path from base_dir for display
        let display_path = if path.starts_with(base_dir) {
            path.strip_prefix(base_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string()
        } else {
            path.to_string_lossy().to_string()
        };

        suggestions.push(Suggestion {
            _id: if is_dir {
                format!("dir-{}", display_path)
            } else {
                format!("file-{}", display_path)
            },
            display_text: if is_dir {
                format!("{}{}", display_path, "/")
            } else {
                display_path
            },
            _description: if is_dir {
                Some("directory".to_string())
            } else {
                None
            },
            suggestion_type: if is_dir {
                SuggestionType::Directory
            } else {
                SuggestionType::File
            },
        });
        }
    }

    // Sort: directories first, then files, alphabetically
    suggestions.sort_by(|a, b| {
        match (&a.suggestion_type, &b.suggestion_type) {
            (SuggestionType::Directory, SuggestionType::File) => std::cmp::Ordering::Less,
            (SuggestionType::File, SuggestionType::Directory) => std::cmp::Ordering::Greater,
            _ => a.display_text.cmp(&b.display_text),
        }
    });

    // Limit to 15 suggestions
    suggestions.truncate(15);
    suggestions
}

/// Generate suggestions based on context
pub fn generate_suggestions(ctx: &SuggestionContext) -> Vec<Suggestion> {
    if let Some((token, _start_pos, trigger_type)) = extract_completion_token(&ctx.input, ctx.cursor_offset) {
        match trigger_type {
            TriggerType::SlashCommand => generate_command_suggestions(&token, &ctx.skills),
            TriggerType::AtSymbol => generate_file_suggestions(&token, &ctx.cwd),
            TriggerType::ModelArg => generate_model_suggestions(&token, &ctx.models),
            TriggerType::ModelSubCommand => generate_model_subcommand_suggestions(&token),
        }
    } else {
        Vec::new()
    }
}

/// Apply a suggestion to the input
/// Returns the new input and new cursor position
pub fn apply_suggestion(
    input: &str,
    cursor_offset: usize,
    suggestion: &Suggestion,
) -> (String, usize) {
    if let Some((_token, start_pos, trigger_type)) = extract_completion_token(input, cursor_offset) {
        let before = &input[..start_pos];
        let after = &input[cursor_offset..];
        
        let replacement = match trigger_type {
            TriggerType::SlashCommand => {
                // For commands, add a space after
                format!("{} ", suggestion.display_text)
            },
            TriggerType::AtSymbol => {
                // For files, add @ prefix and space (for files) or / (for directories)
                match suggestion.suggestion_type {
                    SuggestionType::Directory => {
                        // Directory: keep the trailing /
                        format!("@{}", suggestion.display_text)
                    },
                    _ => {
                        // File: add space
                        format!("@{} ", suggestion.display_text)
                    }
                }
            },
            TriggerType::ModelArg => {
                // For model args, just insert the text
                suggestion.display_text.clone()
            },
            TriggerType::ModelSubCommand => {
                // For model subcommands, add the subcommand after "/model "
                format!("{}", suggestion.display_text)
            },
        };

        let new_input = format!("{}{}{}", before, replacement, after);
        let new_cursor = start_pos + replacement.len();
        
        (new_input, new_cursor)
    } else {
        (input.to_string(), cursor_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_slash_command_token() {
        let input = "/hel";
        let result = extract_completion_token(input, 4);
        // Token should be the full command including /
        assert_eq!(result, Some(("/hel".to_string(), 0, TriggerType::SlashCommand)));
    }

    #[test]
    fn test_extract_at_token() {
        let input = "@src";
        let result = extract_completion_token(input, 4);
        // Token should be @src (including @)
        assert!(result.is_some());
        if let Some((token, pos, trigger)) = result {
            assert_eq!(pos, 0);
            assert_eq!(trigger, TriggerType::AtSymbol);
            assert!(token.starts_with('@'));
        }
    }

    #[test]
    fn test_generate_command_suggestions() {
        let suggestions = generate_command_suggestions("/hel", &[]);
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].display_text, "/help");
    }

    #[test]
    fn test_generate_command_suggestions_empty() {
        let suggestions = generate_command_suggestions("", &[]);
        assert!(suggestions.len() > 5); // Should return all commands
    }

    #[test]
    fn test_generate_command_suggestions_with_skills() {
        let skills = vec![
            ("cm".to_string(), "commit message".to_string(), vec!["commit".to_string()]),
            ("review".to_string(), "code review".to_string(), vec!["cr".to_string()]),
        ];
        // Empty partial → all commands + all skills
        let suggestions = generate_command_suggestions("", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));
        assert!(suggestions.iter().any(|s| s.display_text == "/review"));
        assert!(suggestions.iter().any(|s| s.display_text == "/help"));

        // Partial "c" → matches /cm (name), /clear (command), /commit (command), /context (command)
        let suggestions = generate_command_suggestions("/c", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));

        // Partial "cr" → matches skill alias "cr" → skill "review"
        let suggestions = generate_command_suggestions("/cr", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/review"));
    }
}