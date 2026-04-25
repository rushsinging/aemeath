use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use crate::path_security::validate_and_normalize_path;
use serde_json::Value;

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str { "Edit" }
    fn description(&self) -> &str {
        "Performs exact string replacements in files.\n\nUsage:\n- You must use your Read tool at least once in the conversation before editing. This tool will error if you attempt an edit without reading the file.\n- When editing text from Read tool output, ensure you preserve the exact indentation (tabs/spaces).\n- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless explicitly required.\n- The edit will FAIL if `old_string` is not unique in the file. Either provide a larger string with more surrounding context to make it unique or use `replace_all` to change every instance of `old_string`.\n- Use `replace_all` for replacing and renaming strings across the file."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Absolute path to the file" },
                "old_string": { "type": "string", "description": "The exact text to replace" },
                "new_string": { "type": "string", "description": "The replacement text" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences (default false)" }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }
    fn is_concurrency_safe(&self) -> bool { false }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p, None => return ToolResult::error("missing required parameter: file_path"),
        };

        // Validate path is within workspace boundary (includes traversal check)
        let path = match validate_and_normalize_path(file_path, &ctx.cwd, ctx.allow_all) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e),
        };

        // Check if file was read first
        if let Ok(read_files) = ctx.read_files.lock() {
            if !read_files.contains(file_path) {
                return ToolResult::error(format!(
                    "You must read {file_path} before editing it. Use the Read tool first."
                ));
            }
        }
        let old_string = match input.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s, None => return ToolResult::error("missing required parameter: old_string"),
        };
        let new_string = match input.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s, None => return ToolResult::error("missing required parameter: new_string"),
        };
        let replace_all = input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
        if !path.exists() { return ToolResult::error(format!("file not found: {file_path}")); }
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c, Err(e) => return ToolResult::error(format!("failed to read file: {e}")),
        };
        if old_string == new_string { return ToolResult::error("old_string and new_string are identical"); }

        // Try exact match first, then fuzzy match (normalize leading whitespace)
        let (matched_old, count, is_fuzzy) = {
            let exact_count = content.matches(old_string).count();
            if exact_count > 0 {
                (old_string.to_string(), exact_count, false)
            } else {
                // Fuzzy: normalize each line's leading whitespace and try to find a match
                match fuzzy_find_in_content(&content, old_string) {
                    Some(actual) => {
                        let c = content.matches(&actual).count();
                        (actual, c, true)
                    }
                    None => (old_string.to_string(), 0, false),
                }
            }
        };

        if count == 0 {
            // Provide helpful context
            let first_line = old_string.lines().next().unwrap_or("").trim();
            let hint = if !first_line.is_empty() {
                let mut nearby = String::new();
                for (i, line) in content.lines().enumerate() {
                    if line.contains(first_line) || (first_line.len() > 10 && line.contains(&first_line[..first_line.len().min(30)])) {
                        let start = i.saturating_sub(2);
                        let end = (i + 3).min(content.lines().count());
                        nearby = content.lines()
                            .enumerate()
                            .skip(start)
                            .take(end - start)
                            .map(|(n, l)| format!("{:>4}  {}", n + 1, l))
                            .collect::<Vec<_>>()
                            .join("\n");
                        break;
                    }
                }
                if nearby.is_empty() {
                    "old_string not found in file. Read the file first to get the exact content.".to_string()
                } else {
                    format!("old_string not found. Similar content near line:\n{}\nPlease read the file and use the exact text.", nearby)
                }
            } else {
                "old_string not found in file. Read the file first to get the exact content.".to_string()
            };
            return ToolResult::error(hint);
        }
        if !replace_all && count > 1 {
            return ToolResult::error(format!("old_string found {count} times. Use replace_all or provide more context to make it unique."));
        }

        // Apply the replacement, adapting new_string indentation if fuzzy matched
        let actual_new = if is_fuzzy {
            adapt_indentation(&matched_old, old_string, new_string)
        } else {
            new_string.to_string()
        };
        let new_content = if replace_all { content.replace(&matched_old, &actual_new) }
        else { content.replacen(&matched_old, &actual_new, 1) };
        match tokio::fs::write(path, &new_content).await {
            Ok(()) => {
                let fuzzy_note = if is_fuzzy { " (fuzzy matched, indentation adapted)" } else { "" };
                ToolResult::success(format!(
                    "replaced {} occurrence(s){} in {file_path}\n---DIFF---\n{}\n---DIFF---\n{}",
                    if replace_all { count } else { 1 },
                    fuzzy_note,
                    matched_old,
                    actual_new,
                ))
            }
            Err(e) => ToolResult::error(format!("failed to write file: {e}")),
        }
    }
}

/// Try to find old_string in content by normalizing leading whitespace.
/// Returns the actual matching substring from content if found.
fn fuzzy_find_in_content(content: &str, old_string: &str) -> Option<String> {
    let old_lines: Vec<&str> = old_string.lines().collect();
    if old_lines.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let first_trimmed = old_lines[0].trim();
    if first_trimmed.is_empty() {
        return None;
    }

    // Find candidate start positions by matching first line (trimmed)
    for (i, content_line) in content_lines.iter().enumerate() {
        if content_line.trim() != first_trimmed {
            continue;
        }

        // Check if subsequent lines match (trimmed)
        if i + old_lines.len() > content_lines.len() {
            continue;
        }

        let mut all_match = true;
        for (j, old_line) in old_lines.iter().enumerate() {
            if content_lines[i + j].trim() != old_line.trim() {
                all_match = false;
                break;
            }
        }

        if all_match {
                // Return the actual content lines (with original indentation)
                // Preserve trailing newline if old_string had one
                let has_trailing_newline = old_string.ends_with('\n');
                let matched = content_lines[i..i + old_lines.len()]
                    .join("\n");
                return if has_trailing_newline {
                    Some(format!("{}\n", matched))
                } else {
                    Some(matched)
                };
            }
    }

    None
}

/// Adapt new_string indentation to match the actual file indentation.
/// Detects the indent difference between what the model sent (old_string)
/// and what was actually in the file (matched_old), then applies the same
/// shift to new_string.
fn adapt_indentation(matched_old: &str, model_old: &str, model_new: &str) -> String {
    // Find indentation of first non-empty line in matched vs model
    let actual_indent = matched_old.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);
    let model_indent = model_old.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    if actual_indent == model_indent {
        return model_new.to_string();
    }

    // Apply indent shift to each line of new_string
    model_new.lines()
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else {
                let line_indent = line.len() - line.trim_start().len();
                let new_indent = if actual_indent > model_indent {
                    line_indent + (actual_indent - model_indent)
                } else {
                    line_indent.saturating_sub(model_indent - actual_indent)
                };
                format!("{}{}", " ".repeat(new_indent), line.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
