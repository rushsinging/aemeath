# Feature #9 Real LLM Reflection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `/reflect` perform a real LLM reflection call, show parsed reflection output, keep suggestions pending, and let `/reflect apply` write accepted suggestions into Memory.

**Architecture:** Keep `aemeath-core` responsible for reflection data models, prompt construction, JSON extraction/parsing, formatting, and apply logic. Put the actual LLM call in `aemeath-cli` because it owns `LlmClient`, current messages, system prompt state, and TUI output. Slash command registry remains the fallback path for pure core commands, but `/reflect` becomes a first-class TUI command so it can access runtime LLM state.

**Tech Stack:** Rust, Tokio, `aemeath-core` command/memory/reflection modules, `aemeath-cli` TUI slash handling, `aemeath-llm::LlmClient::stream_message_raw`, serde JSON.

---

## File Map

- Modify `aemeath-core/src/reflection/mod.rs`
  - Add JSON extraction helper for fenced or prose-wrapped LLM output.
  - Add suggestion application helper that converts `MemorySuggestion` into `MemoryEntry` with `MemorySource::Llm`.
  - Add tests for valid JSON, fenced JSON, malformed JSON, applying suggestions, and marking outdated memory.

- Modify `aemeath-core/src/command/commands/reflect.rs`
  - Keep lightweight registry command for non-TUI contexts.
  - Change `/reflect apply` fallback so it no longer deletes outdated memories; outdated marking belongs to reflection apply logic.

- Modify `aemeath-cli/src/tui/app/mod.rs`
  - Add `pending_reflection: Option<ReflectionOutput>` field.
  - Clear it in `reset_runtime_state()`.

- Modify `aemeath-cli/src/tui/app/slash.rs`
  - Intercept `/reflect` before `CommandRegistry`.
  - Implement `/reflect` real LLM call using current client and recent messages.
  - Implement `/reflect apply` by applying pending suggestions/outdated IDs to `MemoryStore`.
  - Respect `memory.reflection.auto_apply_suggestions`.
  - Keep `stats/history` as polish-phase messages.

- Tests:
  - Core tests in `aemeath-core/src/reflection/mod.rs`.
  - CLI compile/check validation. Direct LLM integration is not unit-tested with network calls.

---

## Task 1: Core reflection parsing and apply helpers

**Files:**
- Modify: `aemeath-core/src/reflection/mod.rs`

- [ ] **Step 1: Add failing tests for JSON extraction and apply helpers**

Append these tests inside the existing `#[cfg(test)] mod tests` in `aemeath-core/src/reflection/mod.rs`:

```rust
    #[test]
    fn test_parse_output_extracts_fenced_json() {
        let text = r#"这里是反思结果：
```json
{"deviations":["偏差"],"suggested_memories":[],"outdated_memories":[],"user_alert":null}
```
"#;

        let output = ReflectionEngine::parse_output(text).unwrap();

        assert_eq!(output.deviations, vec!["偏差"]);
    }

    #[test]
    fn test_parse_output_extracts_object_from_prose() {
        let text = r#"反思如下：{"deviations":["遗漏测试"],"suggested_memories":[]}请确认。"#;

        let output = ReflectionEngine::parse_output(text).unwrap();

        assert_eq!(output.deviations, vec!["遗漏测试"]);
    }

    #[test]
    fn test_apply_suggestions_adds_llm_project_memory() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path(), "project", 10, 0.9).unwrap();
        let output = ReflectionOutput {
            deviations: Vec::new(),
            suggested_memories: vec![MemorySuggestion {
                category: crate::memory::MemoryCategory::Decision,
                content: "Reflection 使用真实 LLM 调用".to_string(),
                tags: vec!["reflection".to_string()],
                reason: "用户选择方案 B".to_string(),
            }],
            outdated_memories: Vec::new(),
            user_alert: None,
        };

        let added = ReflectionEngine::apply_suggestions(&output, &mut store).unwrap();
        let memories = store.list(Some(crate::memory::MemoryLayer::Project)).unwrap();

        assert_eq!(added, 1);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source, crate::memory::MemorySource::Llm);
        assert_eq!(memories[0].tags, vec!["reflection"]);
        assert!(memories[0].source_ref.as_deref().unwrap().contains("用户选择方案 B"));
    }

    #[test]
    fn test_apply_output_marks_outdated_and_adds_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path(), "project", 10, 0.9).unwrap();
        let existing = MemoryEntry::new(
            crate::memory::MemoryLayer::Project,
            crate::memory::MemoryCategory::Fact,
            "旧事实",
            crate::memory::MemorySource::User,
        );
        let existing_id = existing.id.clone();
        store.add(existing).unwrap();
        let output = ReflectionOutput {
            deviations: Vec::new(),
            suggested_memories: vec![MemorySuggestion {
                category: crate::memory::MemoryCategory::Pattern,
                content: "先写测试再实现".to_string(),
                tags: Vec::new(),
                reason: String::new(),
            }],
            outdated_memories: vec![existing_id.clone()],
            user_alert: None,
        };

        let applied = ReflectionEngine::apply_output(&output, &mut store).unwrap();
        let memories = store.list(Some(crate::memory::MemoryLayer::Project)).unwrap();
        let outdated = memories.iter().find(|entry| entry.id == existing_id).unwrap();

        assert_eq!(applied.suggestions_added, 1);
        assert_eq!(applied.outdated_marked, 1);
        assert!(outdated.outdated);
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo test -p aemeath-core reflection
```

Expected: FAIL because `apply_suggestions` and `apply_output` do not exist, and `parse_output` does not extract wrapped JSON.

- [ ] **Step 3: Implement minimal core helpers**

In `aemeath-core/src/reflection/mod.rs`, update imports:

```rust
use crate::memory::{AddResult, MemoryCategory, MemoryEntry, MemoryLayer, MemorySource, MemoryStore};
```

Add this struct near `ReflectionOutput`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReflectionApplyResult {
    pub suggestions_added: usize,
    pub outdated_marked: usize,
}
```

Replace `parse_output` implementation and add helper methods inside `impl ReflectionEngine`:

```rust
    pub fn parse_output(json: &str) -> ReflectionResult<ReflectionOutput> {
        let extracted = Self::extract_json_object(json).unwrap_or(json);
        serde_json::from_str(extracted).map_err(ReflectionError::InvalidJson)
    }

    pub fn extract_json_object(text: &str) -> Option<&str> {
        let trimmed = text.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            return Some(trimmed);
        }

        if let Some(fence_start) = trimmed.find("```") {
            let after_start = &trimmed[fence_start + 3..];
            let after_lang = after_start
                .strip_prefix("json")
                .or_else(|| after_start.strip_prefix("JSON"))
                .unwrap_or(after_start)
                .trim_start_matches(['\r', '\n']);
            if let Some(fence_end) = after_lang.find("```") {
                let fenced = after_lang[..fence_end].trim();
                if fenced.starts_with('{') && fenced.ends_with('}') {
                    return Some(fenced);
                }
            }
        }

        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        if start < end {
            Some(trimmed[start..=end].trim())
        } else {
            None
        }
    }

    pub fn apply_suggestions(
        output: &ReflectionOutput,
        store: &mut MemoryStore,
    ) -> ReflectionResult<usize> {
        let mut added = 0;
        for suggestion in &output.suggested_memories {
            let mut entry = MemoryEntry::new(
                MemoryLayer::Project,
                suggestion.category,
                suggestion.content.clone(),
                MemorySource::Llm,
            )
            .with_tags(suggestion.tags.clone());
            if !suggestion.reason.trim().is_empty() {
                entry = entry.with_source_ref(format!("reflection: {}", suggestion.reason));
            } else {
                entry = entry.with_source_ref("reflection");
            }
            match store
                .add(entry)
                .map_err(|error| ReflectionError::Memory(error.to_string()))?
            {
                AddResult::Added | AddResult::Merged { .. } => added += 1,
                AddResult::NeedsEviction { .. } => {
                    return Err(ReflectionError::Memory(
                        "Memory 已满，请先执行 /memory compact 后再应用反思建议".to_string(),
                    ));
                }
            }
        }
        Ok(added)
    }

    pub fn apply_output(
        output: &ReflectionOutput,
        store: &mut MemoryStore,
    ) -> ReflectionResult<ReflectionApplyResult> {
        let outdated_marked = Self::apply_outdated(output, store)?;
        let suggestions_added = Self::apply_suggestions(output, store)?;
        Ok(ReflectionApplyResult {
            suggestions_added,
            outdated_marked,
        })
    }
```

- [ ] **Step 4: Run core reflection tests**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo test -p aemeath-core reflection
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add aemeath-core/src/reflection/mod.rs
git commit -m "feat(reflection): parse and apply LLM reflection output"
```

---

## Task 2: Add pending reflection state to TUI App

**Files:**
- Modify: `aemeath-cli/src/tui/app/mod.rs`

- [ ] **Step 1: Add App state field**

In `App` struct, after `memory_config`, add:

```rust
      /// Pending LLM reflection output waiting for `/reflect apply`.
      pub pending_reflection: Option<aemeath_core::reflection::ReflectionOutput>,
```

- [ ] **Step 2: Initialize field in `App::new`**

After `memory_config: aemeath_core::config::MemoryConfig::default(),`, add:

```rust
              pending_reflection: None,
```

- [ ] **Step 3: Clear field in `reset_runtime_state`**

After `self.ask_user_state = None;`, add:

```rust
          self.pending_reflection = None;
```

- [ ] **Step 4: Run compile check**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo check -p aemeath-cli
```

Expected: PASS, allowing the existing `Cmd::Batch` dead-code warning.

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/tui/app/mod.rs
git commit -m "feat(reflection): track pending reflection in TUI"
```

---

## Task 3: Implement real LLM `/reflect` in TUI slash handler

**Files:**
- Modify: `aemeath-cli/src/tui/app/slash.rs`

- [ ] **Step 1: Add imports**

At the top of `aemeath-cli/src/tui/app/slash.rs`, add imports beside the existing imports:

```rust
use aemeath_core::memory::{MemoryEntry, MemoryLayer};
use aemeath_core::reflection::{ReflectionEngine, ReflectionOutput};
use aemeath_llm::types::SystemBlock;
use tokio_util::sync::CancellationToken;
```

- [ ] **Step 2: Intercept `/reflect` before registry fallback**

In `handle_slash_command`, after the `/context` branch and before `/paste`, add:

```rust
              cmd if cmd == format!("/{}", cmd::REFLECT) => {
                  let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                  self.handle_reflect_command(&args).await;
              }
```

- [ ] **Step 3: Add helper methods inside `impl super::App`**

Add these methods before `update_suggestions` in the same `impl super::App` block:

```rust
      async fn handle_reflect_command(&mut self, args: &str) {
          if !self.memory_config.reflection.enabled {
              self.output_area.push_error("Reflection 系统已禁用。");
              return;
          }

          match args.trim() {
              "" => self.run_llm_reflection().await,
              "apply" => self.apply_pending_reflection(),
              "stats" | "history" => self
                  .output_area
                  .push_system("Reflection stats/history 将在打磨阶段支持。"),
              other => self
                  .output_area
                  .push_error(&format!("未知 reflect 子命令: {other}")),
          }
      }

      async fn run_llm_reflection(&mut self) {
          let Some(client) = self.client.clone() else {
              self.output_area.push_error("当前没有可用的 LLM client，无法执行 Reflection。");
              return;
          };

          let mut store = match self.open_reflection_memory_store() {
              Ok(store) => store,
              Err(error) => {
                  self.output_area.push_error(&error);
                  return;
              }
          };

          let memories = match store.list(Some(MemoryLayer::Project)) {
              Ok(memories) => memories,
              Err(error) => {
                  self.output_area.push_error(&error.to_string());
                  return;
              }
          };
          let project_memory = ReflectionEngine::memory_summary(&memories);
          let recent_summary = ReflectionEngine::recent_messages_summary(&self.messages, 6000);
          let prompt = ReflectionEngine::build_prompt(&project_memory, &recent_summary);
          let messages = vec![aemeath_core::message::Message::user(prompt)];
          let system = vec![SystemBlock::dynamic(
              "你是 Aemeath 的 Reflection 子系统。只输出 JSON，不要输出 Markdown 或解释。".to_string(),
          )];
          let cancel = CancellationToken::new();

          self.output_area.push_system("[reflection: calling LLM...]");
          let response = match client
              .stream_message_raw(&system, &messages, &[], Box::new(|_| {}), &cancel)
              .await
          {
              Ok(response) => response,
              Err(error) => {
                  self.output_area
                      .push_error(&format!("Reflection LLM 调用失败: {error}"));
                  return;
              }
          };

          let text = response.assistant_message.text_content();
          let output = match ReflectionEngine::parse_output(&text) {
              Ok(output) => output,
              Err(error) => {
                  self.output_area
                      .push_error(&format!("Reflection 输出解析失败: {error}"));
                  return;
              }
          };

          let formatted = ReflectionEngine::format_output(&output);
          self.output_area.push_system(&formatted);

          if self.memory_config.reflection.auto_apply_suggestions {
              self.apply_reflection_output(output);
          } else {
              let suggestion_count = output.suggested_memories.len();
              let outdated_count = output.outdated_memories.len();
              self.pending_reflection = Some(output);
              if suggestion_count > 0 || outdated_count > 0 {
                  self.output_area.push_system(&format!(
                      "[reflection: {suggestion_count} 条建议记忆、{outdated_count} 条过时标记待应用；运行 /reflect apply]"
                  ));
              }
          }
      }

      fn apply_pending_reflection(&mut self) {
          let Some(output) = self.pending_reflection.clone() else {
              self.output_area.push_system("没有待应用的 Reflection 建议。");
              return;
          };

          if self.apply_reflection_output(output) {
              self.pending_reflection = None;
          }
      }

      fn apply_reflection_output(&mut self, output: ReflectionOutput) -> bool {
          let mut store = match self.open_reflection_memory_store() {
              Ok(store) => store,
              Err(error) => {
                  self.output_area.push_error(&error);
                  return false;
              }
          };

          match ReflectionEngine::apply_output(&output, &mut store) {
              Ok(applied) => {
                  self.output_area.push_system(&format!(
                      "[reflection applied: 新增/合并 {} 条记忆，标记 {} 条过时记忆]",
                      applied.suggestions_added, applied.outdated_marked
                  ));
                  true
              }
              Err(error) => {
                  self.output_area
                      .push_error(&format!("应用 Reflection 建议失败: {error}"));
                  false
              }
          }
      }

      fn open_reflection_memory_store(&self) -> Result<aemeath_core::memory::MemoryStore, String> {
          let base_dir = aemeath_core::memory::memory_base_dir();
          let project_hash = aemeath_core::memory::project_hash(&self.cwd);
          aemeath_core::memory::MemoryStore::new(
              base_dir,
              project_hash,
              self.memory_config.max_entries,
              self.memory_config.similarity_threshold,
          )
          .map_err(|error| error.to_string())
      }
```

- [ ] **Step 4: Remove unused imports if compiler reports them**

If `MemoryEntry` is unused, remove it from the import so the import becomes:

```rust
use aemeath_core::memory::MemoryLayer;
```

- [ ] **Step 5: Run compile check**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo check -p aemeath-cli
```

Expected: PASS, allowing the existing `Cmd::Batch` dead-code warning.

- [ ] **Step 6: Commit**

```bash
git add aemeath-cli/src/tui/app/slash.rs
git commit -m "feat(reflection): run real LLM reflection from TUI"
```

---

## Task 4: Fix core `/reflect apply` fallback semantics

**Files:**
- Modify: `aemeath-core/src/command/commands/reflect.rs`

- [ ] **Step 1: Add failing test for fallback apply not deleting outdated memory**

Inside `#[cfg(test)] mod tests` in `aemeath-core/src/command/commands/reflect.rs`, add:

```rust
    #[test]
    fn test_build_lightweight_output_does_not_delete_outdated_memory() {
        let mut entry = MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "旧决策",
            MemorySource::User,
        );
        entry.outdated = true;

        let output = build_lightweight_output(&[entry]);

        assert_eq!(output.outdated_memories.len(), 1);
    }
```

This documents that fallback `/reflect` only reports outdated memory. The real TUI `/reflect apply` handles marking/applying pending LLM output.

- [ ] **Step 2: Change fallback `apply_reflection` message**

Replace the body of `apply_reflection` with:

```rust
fn apply_reflection(ctx: &CommandContext, _auto: bool) -> CommandResult {
    if let Err(error) = open_memory_store(ctx) {
        return CommandResult::Error(error);
    }

    CommandResult::Success(
        "核心命令模式没有待应用的 Reflection 建议；请在 TUI 中运行 /reflect 后再执行 /reflect apply。".to_string(),
    )
}
```

- [ ] **Step 3: Run command tests**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo test -p aemeath-core command::commands::reflect
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add aemeath-core/src/command/commands/reflect.rs
git commit -m "fix(reflection): avoid deleting memory in fallback apply"
```

---

## Task 5: Update feature tracking and run final verification

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update Feature #9 row**

Change the Feature #9 row to state that real LLM reflection is implemented and pending/apply is available, while automatic N-turn/PostCompact triggers remain future work.

Use this wording:

```markdown
| 9 | 反思系统 | - | 实施中 | 未确认 | 已接入真实 LLM `/reflect`、JSON 解析、pending 建议与 `/reflect apply` 写入 Memory；自动 N 轮触发与 PostCompact 触发待继续。详见 [spec](specs/009-reflection-system.md) |
```

- [ ] **Step 2: Run focused verification**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && cargo test -p aemeath-core reflection && cargo test -p aemeath-core command::commands::reflect && cargo check -p aemeath-cli && cargo check
```

Expected: all commands PASS. The existing `Cmd::Batch` warning may appear.

- [ ] **Step 3: Check formatting whitespace**

Run:

```bash
cd "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-8-9-memory-reflection" && git diff --check
```

Expected: no output.

- [ ] **Step 4: Commit docs update**

```bash
git add docs/feature/active.md
git commit -m "docs(feature): update reflection implementation status"
```

---

## Self-Review

- Spec coverage:
  - Real LLM Reflection call: Task 3.
  - ReflectionOutput JSON parsing, including wrapped LLM text: Task 1.
  - Display to output area: Task 3 uses `ReflectionEngine::format_output` and `output_area.push_system`.
  - Suggested memories default pending: Task 2 and Task 3.
  - `/reflect apply` writes suggestions to Memory: Task 1 and Task 3.
  - Outdated memories marked, not deleted: Task 1 and Task 4.
  - `auto_apply_suggestions`: Task 3.
  - stats/history deferred: Task 3 keeps explicit polish-phase message.
  - Automatic N-turn and PostCompact triggers: intentionally out of this plan per user instruction to skip Hook and complete via Reflection first; tracked as remaining work.

- Placeholder scan: no TBD/TODO placeholders; every code-changing step has concrete code.
- Type consistency: `ReflectionOutput`, `MemorySuggestion`, `ReflectionEngine`, `MemoryStore`, and `MemoryLayer` match existing code; `Message::user`, `SystemBlock::dynamic`, and `stream_message_raw` match existing APIs.
