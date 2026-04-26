use crate::tui::output_area::{display, build_diff_lines, OutputLine, LineStyle, INDENT};

fn debug_log(msg: &str) {
    use std::io::Write;
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".aemeath")
        .join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Format a tool call for human-friendly display.
pub fn format_tool_call(name: &str, raw_json: &str) -> (String, Vec<String>) {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);

    match name {
        "Bash" => {
            if let Ok(v) = &parsed {
                let cmd = v.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                let timeout = v.get("timeout").and_then(|t| t.as_u64());
                let max_cmd_width = 120usize.saturating_sub(INDENT.len() + 2);
                let truncated = display::truncate_unicode_width(cmd, max_cmd_width);
                let mut detail = format!("$ {truncated}");
                if let Some(t) = timeout {
                    if t != 120_000 {
                        detail.push_str(&format!("  (timeout: {}s)", t / 1000));
                    }
                }
                return (format!("● Bash"), vec![detail]);
            }
        }
        "Read" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let offset = v.get("offset").and_then(|o| o.as_u64());
                let limit = v.get("limit").and_then(|l| l.as_u64());
                let mut detail = format!("Read {path}");
                if let Some(o) = offset {
                    detail.push_str(&format!(" (offset: {o}"));
                    if let Some(l) = limit {
                        detail.push_str(&format!(", limit: {l}"));
                    }
                    detail.push(')');
                }
                return (format!("● Read({path})"), vec![detail]);
            }
        }
        "Write" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
                return (format!("● Write({path})"), vec![format!("{} bytes", content.len())]);
            }
        }
        "Edit" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let old = v.get("old_string").and_then(|s| s.as_str()).unwrap_or("");
                let new = v.get("new_string").and_then(|s| s.as_str()).unwrap_or("");
                let old_lines = old.lines().count();
                let new_lines = new.lines().count();
                let detail = if old_lines == new_lines {
                    format!("Changed {} -> {} chars", old.len(), new.len())
                } else if new_lines > old_lines {
                    format!("Added {} line(s), {} -> {} chars", new_lines - old_lines, old.len(), new.len())
                } else {
                    format!("Removed {} line(s), {} -> {} chars", old_lines - new_lines, old.len(), new.len())
                };
                return (format!("● Edit({path})"), vec![detail]);
            }
        }
        "Glob" => {
            if let Ok(v) = &parsed {
                let pattern = v.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
                return (format!("● Glob({pattern})"), vec![]);
            }
        }
        "Grep" => {
            if let Ok(v) = &parsed {
                let pattern = v.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
                let path = v.get("path").and_then(|p| p.as_str()).unwrap_or(".");
                return (format!("● Grep /{pattern}/"), vec![format!("in {path}")]);
            }
        }
        "Agent" => {
            if let Ok(v) = &parsed {
                let desc = v.get("description").and_then(|d| d.as_str()).unwrap_or("sub-task");
                let role = v.get("role").and_then(|r| r.as_str());
                let model = v.get("model").and_then(|m| m.as_str());
                let prompt = v.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
                let preview = if prompt.len() > 300 {
                    let end = prompt.char_indices()
                        .take_while(|(i, _)| *i < 300)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(300);
                    format!("{}...", &prompt[..end])
                } else {
                    prompt.to_string()
                };
                let mut details = vec![preview];
                if let Some(r) = role {
                    details.push(format!("role: {}", r));
                }
                if let Some(m) = model {
                    details.push(format!("model: {}", m));
                }
                return (format!("● Agent({desc})"), details);
            }
        }
        "WebFetch" => {
            if let Ok(v) = &parsed {
                let url = v.get("url").and_then(|u| u.as_str()).unwrap_or("?");
                return (format!("● WebFetch({url})"), vec![]);
            }
        }
        "TaskCreate" => {
            if let Ok(v) = &parsed {
                let subject = v.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
                return (format!("● TaskCreate({subject})"), vec![]);
            }
        }
        "TaskUpdate" => {
            if let Ok(v) = &parsed {
                let id = v.get("taskId").and_then(|s| s.as_str()).unwrap_or("?");
                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                return (format!("● TaskUpdate({id})"), vec![format!("-> {status}")]);
            }
        }
        "TaskList" => {
            return (format!("● TaskList"), vec![]);
        }
        "Skill" => {
            if let Ok(v) = &parsed {
                let skill = v.get("skill").and_then(|s| s.as_str()).unwrap_or("?");
                return (format!("● Skill({skill})"), vec![]);
            }
        }
        "LSP" => {
            if let Ok(v) = &parsed {
                let op = v.get("operation").and_then(|o| o.as_str()).unwrap_or("?");
                let path = v.get("filePath").and_then(|p| p.as_str()).unwrap_or("?");
                return (format!("● LSP::{op}({path})"), vec![]);
            }
        }
        "TodoWrite" => {
            debug_log(&format!("TodoWrite raw_json: {raw_json}"));
            if let Ok(v) = &parsed {
                if let Some(todos) = v.get("todos").and_then(|t| t.as_array()) {
                    let count = todos.len();
                    let mut details: Vec<String> = Vec::new();
                    for todo in todos.iter().take(3) {
                        let subject = todo.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
                        let status = todo.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
                        let icon = match status {
                            "completed" => "✓",
                            "in_progress" => "◐",
                            _ => "○",
                        };
                        details.push(format!("{icon} {subject}"));
                    }
                    if count > 3 {
                        details.push(format!("... +{} more", count - 3));
                    }
                    return (format!("● TodoWrite ({count} items)"), details);
                }
            }
        }
        "TodoRun" => {
            if let Ok(v) = &parsed {
                if let Some(pending) = v.get("_pending").and_then(|p| p.as_array()) {
                    let mut details = Vec::new();
                    for item in pending {
                        if let Some(s) = item.as_str() {
                            details.push(format!("○ {}", s));
                        }
                    }
                    if !details.is_empty() {
                        return (format!("● TodoRun ({} todo{})", details.len(), if details.len() > 1 { "s" } else { "" }), details);
                    }
                }
                return ("● TodoRun".to_string(), vec!["execute all pending todos".to_string()]);
            }
        }
        _ => {}
    }

    let truncated = if raw_json.len() > 100 {
        let end = raw_json.char_indices()
            .take_while(|(i, _)| *i < 100)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(100);
        format!("{}...", &raw_json[..end])
    } else {
        raw_json.to_string()
    };
    (format!("● {name}"), vec![truncated])
}

impl super::OutputArea {
    /// 流式过程中 tool_use_start 时推送预占 header，立刻让用户看到 tool 被调用
    pub fn push_tool_call_start(&mut self, name: &str) {
        self.finish_streaming();
        self.push_line(OutputLine {
            content: format!("● {name}..."),
            style: LineStyle::ToolCallRunning,
            tool_id: Some(format!("pending:{name}")),
        });
    }

    pub fn push_tool_call(&mut self, tool_id: &str, name: &str, summary: &str) {
        self.finish_streaming();

        // 清除该 tool 的预占 header（如果有）
        let pending_id = format!("pending:{name}");
        if let Some(pos) = self.lines.iter().position(|l| l.tool_id.as_deref() == Some(&pending_id)) {
            self.lines.remove(pos);
        }

        let (header, details) = if name == "TodoWrite" {
            self.format_todowrite(summary)
        } else {
            format_tool_call(name, summary)
        };

        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallRunning,
            tool_id: Some(tool_id.to_string()),
        });

        let detail_style = if name == "Bash" {
            LineStyle::Normal
        } else {
            LineStyle::System
        };
        for detail in details.iter() {
            self.push_line(OutputLine {
                content: format!("{INDENT}{detail}"),
                style: detail_style,
                tool_id: Some(tool_id.to_string()),
            });
        }
    }

    fn format_todowrite(&mut self, raw_json: &str) -> (String, Vec<String>) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);
        debug_log(&format!("TodoWrite raw_json: {raw_json}"));

        if let Ok(v) = parsed {
            if let Some(todos) = v.get("todos").and_then(|t| t.as_array()) {
                let count = todos.len();
                let mut details: Vec<String> = Vec::new();

                for todo in todos.iter() {
                    if let (Some(id), Some(subject)) = (
                        todo.get("id").and_then(|s| s.as_str()),
                        todo.get("subject").and_then(|s| s.as_str()),
                    ) {
                        self.todo_subject_cache.insert(id.to_string(), subject.to_string());
                    }
                }

                for todo in todos.iter().take(3) {
                    let subject = todo.get("subject").and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            todo.get("id").and_then(|s| s.as_str())
                                .and_then(|id| self.todo_subject_cache.get(id).cloned())
                        })
                        .unwrap_or_else(|| "?".to_string());

                    let status = todo.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
                    let icon = match status {
                        "completed" => "✓",
                        "in_progress" => "◐",
                        _ => "○",
                    };
                    details.push(format!("{icon} {subject}"));
                }
                if count > 3 {
                    details.push(format!("... +{} more", count - 3));
                }
                return (format!("● TodoWrite ({count} items)"), details);
            }
        }

        format_tool_call("TodoWrite", raw_json)
    }

    pub fn push_completed_tool_call(&mut self, name: &str, input_json: &str) {
        let (header, details) = format_tool_call(name, input_json);
        let header = header.replacen("●", "✓", 1);
        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallSuccess,
            ..Default::default()
        });
        let detail_style = if name == "Bash" {
            LineStyle::Normal
        } else {
            LineStyle::System
        };
        for detail in details.iter() {
            self.push_line(OutputLine {
                content: format!("{INDENT}{detail}"),
                style: detail_style,
                ..Default::default()
            });
        }
        self.push_line(OutputLine {
            content: String::new(),
            style: LineStyle::System,
            ..Default::default()
        });
    }

    pub fn push_tool_result_with_diff(
        &mut self,
        tool_id: &str,
        tool_name: &str,
        result: &str,
        is_error: bool,
        image_note: &str,
    ) {
        self.finish_streaming();

        let done_icon = if is_error { "✗" } else { "✓" };
        let done_style = if is_error { LineStyle::ToolCallError } else { LineStyle::ToolCallSuccess };

        let mut header_idx: Option<usize> = None;
        for (idx, line) in self.lines.iter_mut().enumerate() {
            if matches!(line.style, LineStyle::ToolCallRunning)
                && line.tool_id.as_deref() == Some(tool_id)
            {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                header_idx = Some(idx);
                break;
            }
        }
        if header_idx.is_none() {
            for (idx, line) in self.lines.iter_mut().enumerate().rev() {
                if matches!(line.style, LineStyle::ToolCallRunning) {
                    line.content = line.content.replacen('●', done_icon, 1);
                    line.style = done_style;
                    header_idx = Some(idx);
                    break;
                }
            }
        }

        let id_tag = Some(tool_id.to_string());
        let mut result_lines: Vec<OutputLine> = Vec::new();

        if is_error {
            result_lines.push(OutputLine {
                content: format!("{INDENT}✗ {result}"),
                style: LineStyle::ToolCallError,
                tool_id: id_tag.clone(),
            });
        } else if tool_name == "Edit" && result.contains("---DIFF---\n") {
            let parts: Vec<&str> = result.splitn(3, "---DIFF---\n").collect();
            if parts.len() == 3 {
                let summary = parts[0].trim();
                build_diff_lines(parts[1], parts[2], &id_tag, &mut result_lines);
                result_lines.push(OutputLine {
                    content: format!("{INDENT}✓ {summary}"),
                    style: LineStyle::ToolCallSuccess,
                    tool_id: id_tag.clone(),
                });
            } else {
                result_lines.push(OutputLine {
                    content: format!("{INDENT}✓ {tool_name} completed"),
                    style: LineStyle::ToolCallSuccess,
                    tool_id: id_tag.clone(),
                });
            }
        } else {
            if !result.trim().is_empty() {
                let max_lines = if matches!(tool_name, "TaskList") { 20 } else { 3 };
                let total = result.lines().count();
                for line in result.lines().take(max_lines) {
                    result_lines.push(OutputLine {
                        content: format!("{INDENT}{line}"),
                        style: LineStyle::System,
                        tool_id: id_tag.clone(),
                    });
                }
                if total > max_lines {
                    result_lines.push(OutputLine {
                        content: format!("{INDENT}... ({} lines omitted)", total - max_lines),
                        style: LineStyle::System,
                        tool_id: id_tag.clone(),
                    });
                }
            }
            result_lines.push(OutputLine {
                content: format!("{INDENT}✓ {tool_name} completed"),
                style: LineStyle::ToolCallSuccess,
                tool_id: id_tag.clone(),
            });
        }

        if !image_note.is_empty() {
            result_lines.push(OutputLine {
                content: image_note.trim().to_string(),
                style: LineStyle::System,
                tool_id: id_tag.clone(),
            });
        }

        result_lines.push(OutputLine {
            content: String::new(),
            style: LineStyle::System,
            tool_id: id_tag.clone(),
        });

        let insert_at = if let Some(start) = header_idx {
            let mut end = start;
            while end + 1 < self.lines.len()
                && self.lines[end + 1].tool_id.as_deref() == Some(tool_id)
            {
                end += 1;
            }
            end + 1
        } else {
            self.lines.len()
        };

        self.insert_lines_at(insert_at, result_lines);
    }
}
