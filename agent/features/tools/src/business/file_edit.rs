use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::edit::{EditInput, EditResult};
use share::tool::{PathAccess, PathKind};

pub struct FileEditTool;

const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "file_path",
    kind: PathKind::File,
}];

#[async_trait]
impl TypedTool for FileEditTool {
    type Output = EditResult;
    fn name(&self) -> &str {
        "Edit"
    }
    fn description(&self) -> &str {
        "Performs exact string replacements in files. Read must be called first. Fails if `old_string` is not unique — use `replace_all` for multiple occurrences."
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        EditInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        EditResult::data_schema()
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &FILE_ACCESS
    }
    fn requires_read_before_write(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<EditResult> {
        let args: EditInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("invalid input: {e}"),
                        "data": null
                    })
                    .to_string(),
                )
            }
        };
        let file_path = args.file_path.as_str();

        // Path has already been validated and normalised by PolicyEngine
        let path = std::path::PathBuf::from(file_path);
        let old_string = args.old_string.as_str();
        let new_string = args.new_string.as_str();
        let replace_all = args.replace_all.unwrap_or(false);
        if !path.exists() {
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("file not found: {file_path}"),
                    "data": null
                })
                .to_string(),
            );
        }
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("failed to read file: {e}"),
                        "data": null
                    })
                    .to_string(),
                )
            }
        };
        if old_string == new_string {
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": "old_string and new_string are identical",
                    "data": null
                })
                .to_string(),
            );
        }

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
                    if line.contains(first_line)
                        || (first_line.len() > 10 && {
                            let trunc: String = first_line.chars().take(30).collect();
                            line.contains(&trunc)
                        })
                    {
                        let start = i.saturating_sub(2);
                        let end = (i + 3).min(content.lines().count());
                        nearby = content
                            .lines()
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
                    "old_string not found in file. Read the file first to get the exact content."
                        .to_string()
                } else {
                    format!("old_string not found. Similar content near line:\n{}\nPlease read the file and use the exact text.", nearby)
                }
            } else {
                "old_string not found in file. Read the file first to get the exact content."
                    .to_string()
            };
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": hint,
                    "data": null
                })
                .to_string(),
            );
        }
        if !replace_all && count > 1 {
            return TypedToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("old_string found {count} times. Use replace_all or provide more context to make it unique."),
                "data": null
            }).to_string());
        }

        // Apply the replacement, adapting new_string indentation if fuzzy matched
        let actual_new = if is_fuzzy {
            adapt_indentation(&matched_old, old_string, new_string)
        } else {
            new_string.to_string()
        };
        let new_content = if replace_all {
            content.replace(&matched_old, &actual_new)
        } else {
            content.replacen(&matched_old, &actual_new, 1)
        };
        match tokio::fs::write(path, &new_content).await {
            Ok(()) => {
                let occurrences = if replace_all { count } else { 1 };
                let diff_start_line = start_line_of_match(&content, &matched_old).unwrap_or(1);
                let data = EditResult {
                    file_path: file_path.to_string(),
                    replacements_made: occurrences as u64,
                    dry_run: false,
                };
                TypedToolResult::success(
                    format!(
                        "Replaced {occurrences} occurrence(s) in {file_path}\n---DIFF:LINE:{diff_start_line}---\n{matched_old}\n---DIFF:LINE:{diff_start_line}---\n{actual_new}"
                    ),
                    data,
                )
            }
            Err(e) => TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("failed to write file: {e}"),
                    "data": null
                })
                .to_string(),
            ),
        }
    }
}

/// Return the 1-based line number where `needle` starts in `content`.
fn start_line_of_match(content: &str, needle: &str) -> Option<usize> {
    let byte_pos = content.find(needle)?;
    Some(content[..byte_pos].lines().count() + 1)
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
            let matched = content_lines[i..i + old_lines.len()].join("\n");
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
    let actual_indent = matched_old
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);
    let model_indent = model_old
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    if actual_indent == model_indent {
        return model_new.to_string();
    }

    // Apply indent shift to each line of new_string
    model_new
        .lines()
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

#[cfg(test)]
#[path = "file_edit_tests.rs"]
mod file_edit_tests;
