# Feature 21 Agent Progress Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 优化 TUI 中 Agent 子任务进度展示，把 `Read, Read, Grep` 这类工具名列表改成按工具+目标摘要分组的紧凑输出，并避免同一 turn 重复刷屏。

**Architecture:** 在 `CliAgentRunner` 侧利用已有 `tool_calls` 的 name/input 生成结构化文本摘要，不改 `AgentRunner` trait 和 `progress_tx: Sender<String>` 协议。TUI `OutputArea::push_tool_progress()` 只负责识别同一 Agent tool 下同一 turn 的进度行并替换，保持兼容普通 progress 文本。

**Tech Stack:** Rust 2024 workspace、tokio mpsc、serde_json、ratatui TUI 输出行模型、cargo test/check。

---

## File Structure

- Modify: `aemeath-cli/src/agent_runner.rs`
  - 新增纯逻辑 helper：`summarize_tool_calls_for_progress()`、`summarize_tool_input()`、`extract_display_path()`、`format_grouped_tool_summaries()`。
  - 替换当前 `[Turn N] calling: Read, Read, Grep` 发送逻辑。
  - 在文件末尾现有 `#[cfg(test)] mod tests` 中增加单元测试。
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`
  - 修改 `push_tool_progress()`，对 `↳ [Turn N] ...` 行按同一 `tool_id + turn` 替换旧行，不再重复追加。
  - 在文件末尾新增 `#[cfg(test)] mod tests` 覆盖替换/兼容行为。
- Modify: `docs/feature/active.md`
  - 开始实现时把 #21 状态改为 `实施中`。
  - 完成实现和验证后把 #21 状态改为 `✅ 已完成`，确认结果仍为 `未确认`。

---

### Task 1: Mark feature #21 in progress

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update feature table status**

Replace the table row for #21 exactly:

```markdown
| 21 | TUI 优化 Agent 调用输出展示 | - | 待实施 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
```

with:

```markdown
| 21 | TUI 优化 Agent 调用输出展示 | - | 实施中 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
```

- [ ] **Step 2: Verify the row changed**

Run:

```bash
git diff -- docs/feature/active.md
```

Expected: only the #21 table row status changes from `待实施` to `实施中`.

---

### Task 2: Add failing tests for AgentRunner tool-call summaries

**Files:**
- Modify: `aemeath-cli/src/agent_runner.rs`

- [ ] **Step 1: Add tests to existing test module**

Append these tests inside the existing `#[cfg(test)] mod tests` in `aemeath-cli/src/agent_runner.rs`, after `test_role_max_tokens_override()`:

```rust
    #[test]
    fn test_summarize_tool_calls_for_progress_groups_duplicate_tools() {
        let calls = vec![
            test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/src/lib.rs"})),
            test_tool_call("2", "Read", serde_json::json!({"file_path": "/repo/src/main.rs"})),
            test_tool_call(
                "3",
                "Grep",
                serde_json::json!({"pattern": "AgentProgress", "path": "/repo/src"}),
            ),
        ];

        let summary = summarize_tool_calls_for_progress(2, &calls);

        assert_eq!(
            summary,
            "[Turn 2] Read ×2: src/lib.rs, src/main.rs | Grep: \"AgentProgress\" in src"
        );
    }

    #[test]
    fn test_summarize_tool_calls_for_progress_truncates_long_groups() {
        let calls = vec![
            test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/a.rs"})),
            test_tool_call("2", "Read", serde_json::json!({"file_path": "/repo/b.rs"})),
            test_tool_call("3", "Read", serde_json::json!({"file_path": "/repo/c.rs"})),
            test_tool_call("4", "Read", serde_json::json!({"file_path": "/repo/d.rs"})),
        ];

        let summary = summarize_tool_calls_for_progress(1, &calls);

        assert_eq!(summary, "[Turn 1] Read ×4: a.rs, b.rs, c.rs +1 more");
    }

    #[test]
    fn test_summarize_tool_calls_for_progress_handles_bash_and_unknown_input() {
        let calls = vec![
            test_tool_call(
                "1",
                "Bash",
                serde_json::json!({"command": "cargo check -p aemeath-cli && cargo test"}),
            ),
            test_tool_call("2", "CustomTool", serde_json::json!({"value": 42})),
        ];

        let summary = summarize_tool_calls_for_progress(3, &calls);

        assert_eq!(
            summary,
            "[Turn 3] Bash: cargo check -p aemeath-cli… | CustomTool: {\"value\":42}"
        );
    }

    fn test_tool_call(id: &str, name: &str, input: serde_json::Value) -> aemeath_core::agent::ToolCall {
        aemeath_core::agent::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }
    }
```

If the actual `ToolCall` type path differs, inspect `Agent::extract_tool_calls()` return type and adjust only the path/import, not the asserted behavior.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_summarize_tool_calls_for_progress -- --nocapture
```

Expected: compilation fails because `summarize_tool_calls_for_progress` is not defined. This is the correct RED state.

---

### Task 3: Implement AgentRunner summary helpers

**Files:**
- Modify: `aemeath-cli/src/agent_runner.rs`

- [ ] **Step 1: Add imports**

At the top of `aemeath-cli/src/agent_runner.rs`, add these imports next to the existing `use` statements:

```rust
use aemeath_core::agent::ToolCall;
use std::collections::BTreeMap;
```

If `ToolCall` is not exported from `aemeath_core::agent`, use the concrete type returned by `Agent::extract_tool_calls()`.

- [ ] **Step 2: Add helper functions before `impl CliAgentRunner`**

Insert this code after the `pub struct CliAgentRunner` definition and before `impl CliAgentRunner`:

```rust
fn summarize_tool_calls_for_progress(turn: usize, tool_calls: &[ToolCall]) -> String {
    let grouped = format_grouped_tool_summaries(tool_calls);
    format!("[Turn {turn}] {grouped}")
}

fn format_grouped_tool_summaries(tool_calls: &[ToolCall]) -> String {
    let mut grouped: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for call in tool_calls {
        grouped
            .entry(call.name.as_str())
            .or_default()
            .push(summarize_tool_input(&call.name, &call.input));
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| {
            let count = summaries.len();
            let mut visible = summaries
                .iter()
                .filter(|summary| !summary.is_empty())
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            if count > 3 {
                visible.push(format!("+{} more", count - 3));
            }
            let suffix = if visible.is_empty() {
                String::new()
            } else {
                format!(": {}", visible.join(", "))
            };
            if count > 1 {
                format!("{name} ×{count}{suffix}")
            } else {
                format!("{name}{suffix}")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" | "Write" | "Edit" | "LSP" => extract_display_path(input, &["file_path", "path"]),
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = extract_display_path(input, &["path"]);
            match (pattern.is_empty(), path.is_empty()) {
                (false, false) => format!("\"{}\" in {}", truncate_progress_part(pattern, 48), path),
                (false, true) => format!("\"{}\"", truncate_progress_part(pattern, 48)),
                (true, false) => path,
                (true, true) => fallback_json_summary(input),
            }
        }
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|pattern| truncate_progress_part(pattern, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|command| truncate_progress_part(command, 32))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|url| truncate_progress_part(url, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskUpdate" | "TaskGet" | "TaskOutput" | "TaskStop" => input
            .get("taskId")
            .and_then(|v| v.as_str())
            .map(|id| truncate_progress_part(id, 48))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskCreate" => input
            .get("subject")
            .and_then(|v| v.as_str())
            .map(|subject| truncate_progress_part(subject, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Memory" => input
            .get("action")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Skill" => input
            .get("skill")
            .and_then(|v| v.as_str())
            .map(|skill| truncate_progress_part(skill, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        _ => fallback_json_summary(input),
    }
}

fn extract_display_path(input: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()))
        .map(|path| {
            let trimmed = path.trim_start_matches("/repo/");
            let components = trimmed.split('/').collect::<Vec<_>>();
            let compact = if components.len() > 3 {
                components[components.len() - 3..].join("/")
            } else {
                trimmed.to_string()
            };
            truncate_progress_part(&compact, 72)
        })
        .unwrap_or_default()
}

fn fallback_json_summary(input: &serde_json::Value) -> String {
    truncate_progress_part(&input.to_string(), 72)
}

fn truncate_progress_part(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let truncated = text.chars().take(max_chars).collect::<String>();
    format!("{truncated}…")
}
```

- [ ] **Step 3: Replace progress emission**

In `run_agent()`, replace this block:

```rust
                      if let Some(ref tx) = progress_tx {
                          let tool_names: Vec<&str> =
                              tool_calls.iter().map(|c| c.name.as_str()).collect();
                          let _ = tx.try_send(format!(
                              "[Turn {}] calling: {}",
                              turn + 1,
                              tool_names.join(", ")
                          ));
                      }
```

with:

```rust
                      if let Some(ref tx) = progress_tx {
                          let _ = tx.try_send(summarize_tool_calls_for_progress(
                              turn + 1,
                              &tool_calls,
                          ));
                      }
```

- [ ] **Step 4: Run tests and verify GREEN**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_summarize_tool_calls_for_progress -- --nocapture
```

Expected: all three new tests pass.

---

### Task 4: Add failing tests for TUI turn progress replacement

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`

- [ ] **Step 1: Add tests at file end**

Append this test module to the end of `aemeath-cli/src/tui/output_area/tool_display.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::OutputArea;

    #[test]
    fn test_push_tool_progress_replaces_same_turn_progress() {
        let mut output = OutputArea::new();

        output.push_tool_progress("agent-1", "[Turn 1] Read: old.rs");
        output.push_tool_progress("agent-1", "[Turn 1] Read: new.rs | Grep: \"needle\"");

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ [Turn 1] Read: new.rs | Grep: \"needle\""]);
    }

    #[test]
    fn test_push_tool_progress_keeps_different_turn_progress() {
        let mut output = OutputArea::new();

        output.push_tool_progress("agent-1", "[Turn 1] Read: a.rs");
        output.push_tool_progress("agent-1", "[Turn 2] Bash: cargo check");

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            matching,
            vec!["  ↳ [Turn 1] Read: a.rs", "  ↳ [Turn 2] Bash: cargo check"]
        );
    }

    #[test]
    fn test_push_tool_progress_keeps_non_turn_progress_compatible() {
        let mut output = OutputArea::new();

        output.push_tool_progress("agent-1", "plain progress");
        output.push_tool_progress("agent-1", "another progress");

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ plain progress", "  ↳ another progress"]);
    }
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test -p aemeath-cli tui::output_area::tool_display::tests::test_push_tool_progress -- --nocapture
```

Expected: `test_push_tool_progress_replaces_same_turn_progress` fails because both same-turn lines are present.

---

### Task 5: Implement same-turn replacement in OutputArea

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`

- [ ] **Step 1: Add helper function before `impl super::OutputArea`**

Insert this function before `impl super::OutputArea` near line 612:

```rust
fn extract_turn_marker(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[Turn ")?;
    let end = rest.find(']')?;
    Some(&text[.."[Turn ".len() + end + 1])
}
```

- [ ] **Step 2: Replace existing same-text dedupe block**

In `push_tool_progress()`, replace this block:

```rust
          let content = format!("{INDENT}↳ {trimmed}");
          let already_shown = self
              .lines
              .iter()
              .rev()
              .take(8)
              .any(|line| line.tool_id.as_deref() == Some(tool_id) && line.content == content);
          if already_shown {
              return;
          }
```

with:

```rust
          let content = format!("{INDENT}↳ {trimmed}");
          if let Some(turn_marker) = extract_turn_marker(trimmed) {
              if let Some(line) = self.lines.iter_mut().rev().find(|line| {
                  line.tool_id.as_deref() == Some(tool_id)
                      && line
                          .content
                          .strip_prefix(&format!("{INDENT}↳ "))
                          .and_then(extract_turn_marker)
                          == Some(turn_marker)
              }) {
                  line.content = content;
                  line.style = LineStyle::System;
                  return;
              }
          }

          let already_shown = self
              .lines
              .iter()
              .rev()
              .take(8)
              .any(|line| line.tool_id.as_deref() == Some(tool_id) && line.content == content);
          if already_shown {
              return;
          }
```

- [ ] **Step 3: Run TUI tests and verify GREEN**

Run:

```bash
cargo test -p aemeath-cli tui::output_area::tool_display::tests::test_push_tool_progress -- --nocapture
```

Expected: all three new tests pass.

---

### Task 6: Run full verification and update #21 completed

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Run targeted tests**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_summarize_tool_calls_for_progress tui::output_area::tool_display::tests::test_push_tool_progress -- --nocapture
```

Expected: new AgentRunner and OutputArea tests pass.

- [ ] **Step 2: Run crate check**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: command exits 0.

- [ ] **Step 3: Update feature #21 status to completed**

Replace the table row for #21 exactly:

```markdown
| 21 | TUI 优化 Agent 调用输出展示 | - | 实施中 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
```

with:

```markdown
| 21 | TUI 优化 Agent 调用输出展示 | - | ✅ 已完成 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
```

- [ ] **Step 4: Record completed changes in detail section**

Because #21 currently only exists in the table, add this detail section after the #18 section and before #4:

```markdown
---

### #21 TUI 优化 Agent 调用输出展示

**目标**：优化 Agent 子任务每个 turn 的工具调用进度展示，避免只显示 `Read, Read, Grep` 这类无目标列表。

**已完成的改动**：

1. **工具调用摘要**：Agent runner 根据 tool call input 生成目标摘要，例如 `Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src`。
2. **同工具分组**：同一 turn 内相同工具按工具名合并，并显示调用次数。
3. **目标提取**：Read/Edit/Write/LSP 显示路径；Grep 显示 pattern + path；Glob 显示 pattern；Bash 显示命令首段；Task/Skill/Memory 显示关键字段。
4. **TUI 去重替换**：同一个 Agent tool 下同一个 `[Turn N]` 的进度更新会替换旧行，不会重复刷屏。
5. **兼容保留**：非 `[Turn N]` 普通 progress 仍按原逻辑追加和去重。

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（Agent tool call progress 摘要生成）
- `aemeath-cli/src/tui/output_area/tool_display.rs`（同 turn progress 替换）

**测试**：新增单元测试覆盖工具分组摘要、长列表截断、Bash/未知工具 fallback、同 turn 替换、不同 turn 保留、普通 progress 兼容。
```

- [ ] **Step 5: Verify docs diff**

Run:

```bash
git diff -- docs/feature/active.md
```

Expected: #21 status is `✅ 已完成` and the new #21 detail section exists.

---

### Task 7: Final review gate

**Files:**
- Review only.

- [ ] **Step 1: Check working tree diff**

Run:

```bash
git diff -- aemeath-cli/src/agent_runner.rs aemeath-cli/src/tui/output_area/tool_display.rs docs/feature/active.md
```

Expected:
- `agent_runner.rs` only adds summary helpers, tests, and replaces the old tool-name-only progress string.
- `tool_display.rs` only adds turn marker replacement logic and tests.
- `docs/feature/active.md` reflects #21 completion.

- [ ] **Step 2: Run final verification**

Run:

```bash
cargo test -p aemeath-cli agent_runner::tests::test_summarize_tool_calls_for_progress tui::output_area::tool_display::tests::test_push_tool_progress -- --nocapture && cargo check -p aemeath-cli
```

Expected: all commands exit 0.

- [ ] **Step 3: Do not archive #21 yet**

Leave #21 in `docs/feature/active.md` with `确认结果` = `未确认`. Per project rules, archive only after the user explicitly confirms completion.
