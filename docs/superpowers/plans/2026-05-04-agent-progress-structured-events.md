# Agent Progress Structured Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Agent progress string protocol with structured events while keeping the TUI display focused on the current Agent activity, not turn numbers.

**Architecture:** Add shared progress event types to `aemeath-core/src/tool.rs`, change `AgentRunner` and `ToolContext` progress channels to `Sender<AgentProgressEvent>`, emit structured `ToolCalls` events from `CliAgentRunner`, and render them in TUI without parsing `[Turn N]` strings. The UI keeps one current tool-call progress line per Agent tool id and appends only plain `Message` events.

**Tech Stack:** Rust workspace, async_trait, tokio mpsc, serde_json, existing TUI `OutputArea` line model, cargo test/check.

---

## File Structure

- Modify: `aemeath-core/src/tool.rs`
  - Add `AgentProgressEvent`, `AgentProgressKind`, `AgentToolCallProgress`.
  - Change `AgentRunner::run_agent()` and `ToolContext.progress_tx` to use `Sender<AgentProgressEvent>`.
- Modify: `aemeath-cli/src/agent_runner.rs`
  - Replace string summary API with structured `AgentProgressEvent` builder.
  - Keep summary extraction helpers for `AgentToolCallProgress.summary`.
  - Update tests to assert structured events.
- Modify: `aemeath-tools/src/agent_tool.rs`
  - No behavior change expected; compile against new `ToolContext.progress_tx` type.
- Modify: `aemeath-cli/src/tui/app/mod.rs`
  - Change `UiEvent::AgentProgress` payload from `text: String` to `event: AgentProgressEvent`.
- Modify: `aemeath-cli/src/tui/app/stream.rs`
  - Change progress channel type and forward structured events.
- Modify: `aemeath-cli/src/tui/app/update.rs`
  - Pass structured progress event to `OutputArea`.
- Modify: `aemeath-cli/src/tui/app/event_handler.rs`
  - Adjust pattern match for the new payload shape if needed.
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`
  - Replace string turn parsing with structured `push_agent_progress()` rendering.
  - Keep `push_tool_progress()` only if needed as a compatibility wrapper; tests should target structured API.
- Modify: `docs/feature/active.md`
  - Update #21 detail to mention structured events instead of string protocol.

---

### Task 1: Add shared Agent progress event types

**Files:**
- Modify: `aemeath-core/src/tool.rs`

- [ ] **Step 1: Insert event structs after `ToolResult` impl**

Add this code after the `impl ToolResult` block and before `AgentRunner`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEvent {
    /// Monotonic sequence for internal ordering/replacement. UI does not display it by default.
    pub sequence: usize,
    pub kind: AgentProgressKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKind {
    ToolCalls { calls: Vec<AgentToolCallProgress> },
    Message { text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgress {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub summary: String,
}
```

- [ ] **Step 2: Change trait and context channel types**

In `AgentRunner::run_agent`, replace:

```rust
          progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
```

with:

```rust
          progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
```

In `ToolContext`, replace:

```rust
    pub progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
```

with:

```rust
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
```

- [ ] **Step 3: Run check and verify expected compile failures**

Run:

```bash
cargo check -p aemeath-core
```

Expected: `aemeath-core` compiles. Later crates may fail until updated.

---

### Task 2: Convert AgentRunner from string summaries to structured events

**Files:**
- Modify: `aemeath-cli/src/agent_runner.rs`

- [ ] **Step 1: Update imports**

Replace:

```rust
use aemeath_core::tool::{AgentRunner, ToolContext, ToolRegistry};
```

with:

```rust
use aemeath_core::tool::{
    AgentProgressEvent, AgentProgressKind, AgentRunner, AgentToolCallProgress, ToolContext,
    ToolRegistry,
};
```

- [ ] **Step 2: Replace string summary function**

Replace the existing `summarize_tool_calls_for_progress()` function:

```rust
fn summarize_tool_calls_for_progress(turn: usize, tool_calls: &[ToolCall]) -> String {
    let grouped = format_grouped_tool_summaries(tool_calls);
    format!("[Turn {turn}] {grouped}")
}
```

with:

```rust
fn build_tool_calls_progress_event(sequence: usize, tool_calls: &[ToolCall]) -> AgentProgressEvent {
    AgentProgressEvent {
        sequence,
        kind: AgentProgressKind::ToolCalls {
            calls: tool_calls
                .iter()
                .map(|call| AgentToolCallProgress {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                    summary: summarize_tool_input(&call.name, &call.input),
                })
                .collect(),
        },
    }
}
```

Keep `format_grouped_tool_summaries()` and summary helpers for UI tests for now; they will move to rendering in Task 5 if needed.

- [ ] **Step 3: Replace progress emission**

Replace:

```rust
                      if let Some(ref tx) = progress_tx {
                          let _ = tx.try_send(summarize_tool_calls_for_progress(
                              turn + 1,
                              &tool_calls,
                          ));
                      }
```

with:

```rust
                      if let Some(ref tx) = progress_tx {
                          let _ = tx.try_send(build_tool_calls_progress_event(turn + 1, &tool_calls));
                      }
```

- [ ] **Step 4: Replace AgentRunner tests**

In the existing `#[cfg(test)] mod tests`, replace the three `test_summarize_tool_calls_for_progress_*` tests with these tests:

```rust
    #[test]
    fn test_build_tool_calls_progress_event_preserves_call_data_and_summaries() {
        let calls = vec![
            test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/src/lib.rs"})),
            test_tool_call(
                "2",
                "Grep",
                serde_json::json!({"pattern": "AgentProgress", "path": "/repo/src"}),
            ),
        ];

        let event = build_tool_calls_progress_event(2, &calls);

        assert_eq!(event.sequence, 2);
        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].id, "1");
                assert_eq!(calls[0].name, "Read");
                assert_eq!(calls[0].input, serde_json::json!({"file_path": "/repo/src/lib.rs"}));
                assert_eq!(calls[0].summary, "src/lib.rs");
                assert_eq!(calls[1].name, "Grep");
                assert_eq!(calls[1].summary, "\"AgentProgress\" in src");
            }
            AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
        }
    }

    #[test]
    fn test_build_tool_calls_progress_event_truncates_long_read_groups_at_summary_level() {
        let calls = vec![test_tool_call(
            "1",
            "Bash",
            serde_json::json!({"command": "cargo check -p aemeath-cli && cargo test"}),
        )];

        let event = build_tool_calls_progress_event(1, &calls);

        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                assert_eq!(calls[0].summary, "cargo check -p aemeath-cli…");
            }
            AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
        }
    }

    #[test]
    fn test_format_grouped_tool_summaries_keeps_existing_display_format() {
        let calls = vec![
            test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/a.rs"})),
            test_tool_call("2", "Read", serde_json::json!({"file_path": "/repo/b.rs"})),
            test_tool_call("3", "Read", serde_json::json!({"file_path": "/repo/c.rs"})),
            test_tool_call("4", "Read", serde_json::json!({"file_path": "/repo/d.rs"})),
        ];

        let summary = format_grouped_tool_summaries(&calls);

        assert_eq!(summary, "Read ×4: a.rs, b.rs, c.rs +1 more");
    }
```

Keep the existing `test_tool_call()` helper.

- [ ] **Step 5: Run tests and verify current crate may still fail from TUI type mismatches**

Run:

```bash
cargo test -p aemeath-cli test_build_tool_calls_progress_event -- --nocapture
```

Expected: may fail to compile until TUI event types are updated in later tasks. If it compiles, the two event tests should pass.

---

### Task 3: Update TUI event and stream forwarding types

**Files:**
- Modify: `aemeath-cli/src/tui/app/mod.rs`
- Modify: `aemeath-cli/src/tui/app/stream.rs`
- Modify: `aemeath-cli/src/tui/app/event_handler.rs`

- [ ] **Step 1: Import structured event type in app module**

In `aemeath-cli/src/tui/app/mod.rs`, replace:

```rust
use aemeath_core::tool::{ImageData, ToolRegistry};
```

with:

```rust
use aemeath_core::tool::{AgentProgressEvent, ImageData, ToolRegistry};
```

- [ ] **Step 2: Change UiEvent payload**

In `UiEvent`, replace:

```rust
    AgentProgress {
        tool_id: String,
        text: String,
    },
```

with:

```rust
    AgentProgress {
        tool_id: String,
        event: AgentProgressEvent,
    },
```

- [ ] **Step 3: Update stream progress channel**

In `aemeath-cli/src/tui/app/stream.rs`, replace:

```rust
                                      let (prog_tx, mut prog_rx) =
                                          tokio::sync::mpsc::channel::<String>(32);
```

with:

```rust
                                      let (prog_tx, mut prog_rx) =
                                          tokio::sync::mpsc::channel::<aemeath_core::tool::AgentProgressEvent>(32);
```

Replace:

```rust
                                          while let Some(text) = prog_rx.recv().await {
                                              let _ = ui_tx
                                                  .send(UiEvent::AgentProgress {
                                                      tool_id: call_id.clone(),
                                                      text,
                                                  })
                                                  .await;
                                          }
```

with:

```rust
                                          while let Some(event) = prog_rx.recv().await {
                                              let _ = ui_tx
                                                  .send(UiEvent::AgentProgress {
                                                      tool_id: call_id.clone(),
                                                      event,
                                                  })
                                                  .await;
                                          }
```

- [ ] **Step 4: Confirm event handler pattern still compiles**

`aemeath-cli/src/tui/app/event_handler.rs` currently matches `UiEvent::AgentProgress { .. }`; keep that wildcard match unchanged.

- [ ] **Step 5: Run check and capture remaining failures**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: remaining errors should point to `update.rs` and `OutputArea` method signatures.

---

### Task 4: Add structured OutputArea tests

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`

- [ ] **Step 1: Replace old string progress tests**

Replace the existing `#[cfg(test)] mod tests` in `tool_display.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::super::OutputArea;
    use aemeath_core::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

    #[test]
    fn test_push_agent_progress_replaces_tool_calls_for_same_agent() {
        let mut output = OutputArea::new();

        output.push_agent_progress("agent-1", tool_calls_event(1, vec![call("1", "Read", "old.rs")]));
        output.push_agent_progress(
            "agent-1",
            tool_calls_event(
                2,
                vec![call("2", "Read", "new.rs"), call("3", "Grep", "\"needle\" in src")],
            ),
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read: new.rs | Grep: \"needle\" in src"]);
    }

    #[test]
    fn test_push_agent_progress_keeps_different_agent_tool_calls_separate() {
        let mut output = OutputArea::new();

        output.push_agent_progress("agent-1", tool_calls_event(1, vec![call("1", "Read", "a.rs")]));
        output.push_agent_progress("agent-2", tool_calls_event(1, vec![call("2", "Bash", "cargo check")]));

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref().is_some())
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read: a.rs", "  ↳ Bash: cargo check"]);
    }

    #[test]
    fn test_push_agent_progress_groups_duplicate_tools_without_showing_turn() {
        let mut output = OutputArea::new();

        output.push_agent_progress(
            "agent-1",
            tool_calls_event(
                7,
                vec![
                    call("1", "Read", "a.rs"),
                    call("2", "Read", "b.rs"),
                    call("3", "Read", "c.rs"),
                    call("4", "Read", "d.rs"),
                ],
            ),
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read ×4: a.rs, b.rs, c.rs +1 more"]);
    }

    #[test]
    fn test_push_agent_progress_appends_message_events() {
        let mut output = OutputArea::new();

        output.push_agent_progress("agent-1", message_event(1, "plain progress"));
        output.push_agent_progress("agent-1", message_event(2, "another progress"));

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ plain progress", "  ↳ another progress"]);
    }

    fn tool_calls_event(sequence: usize, calls: Vec<AgentToolCallProgress>) -> AgentProgressEvent {
        AgentProgressEvent {
            sequence,
            kind: AgentProgressKind::ToolCalls { calls },
        }
    }

    fn message_event(sequence: usize, text: &str) -> AgentProgressEvent {
        AgentProgressEvent {
            sequence,
            kind: AgentProgressKind::Message {
                text: text.to_string(),
            },
        }
    }

    fn call(id: &str, name: &str, summary: &str) -> AgentToolCallProgress {
        AgentToolCallProgress {
            id: id.to_string(),
            name: name.to_string(),
            input: serde_json::json!({}),
            summary: summary.to_string(),
        }
    }
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test -p aemeath-cli test_push_agent_progress -- --nocapture
```

Expected: compile fails because `OutputArea::push_agent_progress` does not exist.

---

### Task 5: Implement structured OutputArea rendering

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/tool_display.rs`
- Modify: `aemeath-cli/src/tui/app/update.rs`

- [ ] **Step 1: Add imports**

At the top of `tool_display.rs`, add `AgentProgressEvent` and `AgentProgressKind` to existing imports. If there is no existing `aemeath_core::tool` import, add:

```rust
use aemeath_core::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};
```

- [ ] **Step 2: Remove string turn marker helper**

Delete the `extract_turn_marker()` helper added by the previous string-protocol implementation:

```rust
fn extract_turn_marker(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[Turn ")?;
    let end = rest.find(']')?;
    Some(&text[.."[Turn ".len() + end + 1])
}
```

- [ ] **Step 3: Add structured grouping helpers before `impl super::OutputArea`**

Insert before `impl super::OutputArea`:

```rust
fn format_agent_tool_calls(calls: &[AgentToolCallProgress]) -> String {
    let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
    for call in calls {
        if let Some((_, summaries)) = grouped.iter_mut().find(|(name, _)| *name == call.name) {
            summaries.push(call.summary.as_str());
        } else {
            grouped.push((call.name.as_str(), vec![call.summary.as_str()]));
        }
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| {
            let count = summaries.len();
            let visible = summaries
                .iter()
                .filter(|summary| !summary.is_empty())
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            let suffix = if visible.is_empty() {
                String::new()
            } else {
                let mut text = visible.join(", ");
                if count > 3 {
                    text.push_str(&format!(" +{} more", count - 3));
                }
                format!(": {text}")
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
```

- [ ] **Step 4: Add `push_agent_progress` method**

Inside `impl super::OutputArea`, add this method near `push_tool_progress()`:

```rust
    pub fn push_agent_progress(&mut self, tool_id: &str, event: AgentProgressEvent) {
        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                let summary = format_agent_tool_calls(&calls);
                let content = format!("{INDENT}↳ {summary}");
                if let Some(line) = self.lines.iter_mut().rev().find(|line| {
                    line.tool_id.as_deref() == Some(tool_id)
                        && line.metadata.as_deref() == Some("agent_tool_calls")
                }) {
                    line.content = content;
                    line.style = LineStyle::System;
                    return;
                }
                self.push_line(OutputLine::with_tool_id(
                    content,
                    LineStyle::System,
                    tool_id.to_string(),
                ).with_metadata("agent_tool_calls".to_string()));
            }
            AgentProgressKind::Message { text } => {
                self.push_tool_progress(tool_id, &text);
            }
        }
    }
```

If `OutputLine` does not have `with_metadata`, inspect `OutputLine` in `types.rs` and set `metadata` directly after constructing the line:

```rust
                let mut line = OutputLine::with_tool_id(
                    content,
                    LineStyle::System,
                    tool_id.to_string(),
                );
                line.metadata = Some("agent_tool_calls".to_string());
                self.push_line(line);
```

Use whichever matches the existing API.

- [ ] **Step 5: Simplify `push_tool_progress` back to plain text compatibility**

In `push_tool_progress()`, remove same-turn replacement logic and keep only exact duplicate dedupe:

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

- [ ] **Step 6: Update TUI update handler**

In `aemeath-cli/src/tui/app/update.rs`, replace:

```rust
              UiEvent::AgentProgress { tool_id, text } => {
                  self.output_area.push_tool_progress(&tool_id, &text);
                  self.output_area.start_spinner();
                  self.output_area.set_spinner_phase("Agent working...");
              }
```

with:

```rust
              UiEvent::AgentProgress { tool_id, event } => {
                  self.output_area.push_agent_progress(&tool_id, event);
                  self.output_area.start_spinner();
                  self.output_area.set_spinner_phase("Agent working...");
              }
```

- [ ] **Step 7: Run tests and verify GREEN for OutputArea**

Run:

```bash
cargo test -p aemeath-cli test_push_agent_progress -- --nocapture
```

Expected: four structured OutputArea tests pass.

---

### Task 6: Remove old string-protocol tests and verify AgentRunner

**Files:**
- Modify: `aemeath-cli/src/agent_runner.rs`

- [ ] **Step 1: Search for old string protocol references**

Run:

```bash
python3 - <<'PY'
from pathlib import Path
root=Path('/Users/guoyuqi/Nextcloud/work/claudecode/aemeath')
for p in root.rglob('*.rs'):
    txt=p.read_text(errors='ignore')
    if 'summarize_tool_calls_for_progress' in txt or '[Turn ' in txt or 'calling:' in txt:
        print(p.relative_to(root))
PY
```

Expected: no `summarize_tool_calls_for_progress`; `[Turn ` should not appear in Agent progress code/tests.

- [ ] **Step 2: Run AgentRunner structured tests**

Run:

```bash
cargo test -p aemeath-cli test_build_tool_calls_progress_event -- --nocapture
```

Expected: two structured event tests pass.

- [ ] **Step 3: Run display format test**

Run:

```bash
cargo test -p aemeath-cli test_format_grouped_tool_summaries_keeps_existing_display_format -- --nocapture
```

Expected: one display-format helper test passes.

---

### Task 7: Update docs for #21 structured events

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Replace #21 detail bullets**

In `docs/feature/active.md`, in section `### #21 TUI 优化 Agent 调用输出展示`, replace bullets 1-5 under `**已完成的改动**` with:

```markdown
1. **结构化事件协议**：Agent progress 从 `Sender<String>` 升级为 `Sender<AgentProgressEvent>`，不再依赖 TUI 解析 `[Turn N]` 文本。
2. **工具调用摘要**：Agent runner 根据 tool call input 生成 `AgentToolCallProgress.summary`，例如 `Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src`。
3. **同工具分组**：TUI 根据结构化 calls 按工具名合并，并显示调用次数；turn/sequence 仅用于内部定位，默认不展示。
4. **当前进度单行更新**：同一个 Agent tool 的 `ToolCalls` 进度只保留一行，新事件替换旧行，不重复刷屏。
5. **兼容保留**：`AgentProgressKind::Message` 用于普通文本 progress，仍按原逻辑追加和去重。
```

- [ ] **Step 2: Replace tests sentence**

Replace:

```markdown
**测试**：新增单元测试覆盖工具分组摘要、长列表截断、Bash/未知工具 fallback、同 turn 替换、不同 turn 保留、普通 progress 兼容。
```

with:

```markdown
**测试**：新增单元测试覆盖结构化事件构造、目标摘要生成、同 Agent 当前进度替换、不同 Agent 互不覆盖、普通 Message progress 兼容。
```

- [ ] **Step 3: Run docs diff check**

Run:

```bash
git diff --check -- docs/feature/active.md docs/superpowers/specs/2026-05-04-agent-progress-structured-events-design.md
```

Expected: exits 0.

---

### Task 8: Final verification

**Files:**
- Review only.

- [ ] **Step 1: Run formatter**

Run:

```bash
cargo fmt
```

Expected: exits 0.

- [ ] **Step 2: Run targeted tests**

Run:

```bash
cargo test -p aemeath-cli test_build_tool_calls_progress_event -- --nocapture && cargo test -p aemeath-cli test_format_grouped_tool_summaries_keeps_existing_display_format -- --nocapture && cargo test -p aemeath-cli test_push_agent_progress -- --nocapture
```

Expected: all targeted tests pass.

- [ ] **Step 3: Run core and CLI checks**

Run:

```bash
cargo check -p aemeath-core && cargo check -p aemeath-cli
```

Expected: both checks exit 0.

- [ ] **Step 4: Run diff check**

Run:

```bash
git diff --check -- aemeath-core/src/tool.rs aemeath-cli/src/agent_runner.rs aemeath-cli/src/tui/app/mod.rs aemeath-cli/src/tui/app/stream.rs aemeath-cli/src/tui/app/update.rs aemeath-cli/src/tui/output_area/tool_display.rs docs/feature/active.md docs/superpowers/specs/2026-05-04-agent-progress-structured-events-design.md
```

Expected: exits 0.

- [ ] **Step 5: Confirm no old Agent progress string protocol remains**

Run:

```bash
python3 - <<'PY'
from pathlib import Path
root=Path('/Users/guoyuqi/Nextcloud/work/claudecode/aemeath')
for p in root.rglob('*.rs'):
    txt=p.read_text(errors='ignore')
    if 'Sender<String>' in txt and 'progress_tx' in txt:
        print('string progress channel remains:', p.relative_to(root))
    if 'summarize_tool_calls_for_progress' in txt:
        print('old summary function remains:', p.relative_to(root))
PY
```

Expected: no output.
