# Compact Total Token Normalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Main and Sub auto-compact decisions use the latest provider-normalized total token count, including Anthropic cache read/create tokens, without changing recent-tail behavior.

**Architecture:** Provider adapters normalize `Usage.total_tokens` at the anti-corruption boundary. Runtime stores only the latest normalized total for compact decisions, Context compares that total with its threshold, and a successful compact resets the value so stale usage cannot retrigger. Existing raw usage fields remain available for cost and diagnostics.

**Tech Stack:** Rust, Tokio, serde_json, workspace crates `provider`, `context`, and `runtime`.

---

### Task 1: Normalize Provider Usage Totals

**Files:**
- Modify: `agent/features/provider/src/domain/invoke.rs`
- Modify: `agent/features/provider/src/adapters/anthropic/message_conversion.rs`
- Modify: `agent/features/provider/src/adapters/stream.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/non_stream.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/stream.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/responses_stream.rs`
- Test: `agent/features/provider/src/domain/invoke.rs`

- [x] **Step 1: Write failing unit tests for provider-family normalization**

Add tests for:

```rust
#[test]
fn openai_total_prefers_reported_total_and_does_not_add_cached_tokens() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 20,
        cached_tokens: Some(80),
        total_tokens: Some(150),
        ..Usage::default()
    };
    assert_eq!(usage.normalized_total_tokens(0), 150);
}

#[test]
fn openai_total_falls_back_to_input_plus_output() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 20,
        cached_tokens: Some(80),
        ..Usage::default()
    };
    assert_eq!(usage.normalized_total_tokens(0), 120);
}

#[test]
fn anthropic_total_includes_cache_read_and_creation_tokens() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 20,
        cached_tokens: Some(80),
        cache_creation_tokens: Some(30),
        ..Usage::default()
    };
    assert_eq!(usage.normalized_total_tokens(110), 230);
}
```

- [x] **Step 2: Run the provider tests and verify RED**

Run:

```bash
cargo test -p provider normalized_total_tokens
```

Expected: compilation fails because `Usage::normalized_total_tokens` does not exist.

- [x] **Step 3: Add the shared normalization helper**

Implement one checked/saturating helper on `Usage`:

```rust
impl Usage {
    pub fn normalized_total_tokens(&self, additional_input_tokens: u32) -> u32 {
        self.total_tokens.unwrap_or_else(|| {
            self.input_tokens
                .saturating_add(additional_input_tokens)
                .saturating_add(self.output_tokens)
        })
    }

    pub fn finalize_total_tokens(&mut self, additional_input_tokens: u32) {
        self.total_tokens = Some(self.normalized_total_tokens(additional_input_tokens));
    }
}
```

OpenAI-compatible adapters call `finalize_total_tokens(0)` after parsing usage. Anthropic adapters call it with:

```rust
usage.cached_tokens.unwrap_or(0)
    .saturating_add(usage.cache_creation_tokens.unwrap_or(0))
```

This preserves OpenAI cached-token semantics and includes both Anthropic cache components.

- [x] **Step 4: Run provider tests and verify GREEN**

Run:

```bash
cargo test -p provider normalized_total_tokens
```

Expected: all matching tests pass.

- [x] **Step 5: Run all provider tests**

Run:

```bash
cargo test -p provider
```

Expected: all provider tests pass.

### Task 2: Make Context Compare a Normalized Total

**Files:**
- Modify: `agent/features/context/src/domain/token_budget.rs`
- Modify: `agent/features/context/src/domain/token_budget_tests.rs`

- [x] **Step 1: Write failing threshold tests**

Add tests that call the intended API directly:

```rust
#[test]
fn normalized_total_above_threshold_needs_compaction() {
    assert!(needs_compaction_total(900_000, 1_048_576));
}

#[test]
fn normalized_total_at_or_below_threshold_does_not_need_compaction() {
    let threshold = autocompact_threshold(1_048_576, 8192) as u64;
    assert!(!needs_compaction_total(threshold, 1_048_576));
}
```

- [x] **Step 2: Run the context tests and verify RED**

Run:

```bash
cargo test -p context needs_compaction_total
```

Expected: compilation fails because `needs_compaction_total` does not exist.

- [x] **Step 3: Implement the provider-neutral comparison**

Add:

```rust
pub fn needs_compaction_total(last_total_tokens: u64, context_size: usize) -> bool {
    let threshold = autocompact_threshold(context_size, 8192) as u64;
    last_total_tokens > threshold
}
```

Retain the existing compatibility helper only if another production caller still needs it; otherwise migrate callers and remove the obsolete multi-field compact decision helper and its misleading cache comments.

- [x] **Step 4: Run context tests and verify GREEN**

Run:

```bash
cargo test -p context needs_compaction_total
```

Expected: both threshold tests pass.

### Task 3: Use Latest Total in Main Runtime

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/compact.rs`
- Test: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

- [x] **Step 1: Write failing pure-decision tests**

Add tests for `should_compact_now(last_total_tokens, context_size, message_count)`:

```rust
#[test]
fn auto_compact_uses_latest_normalized_total() {
    assert!(should_compact_now(Some(900_000), 1_048_576, 5));
}

#[test]
fn auto_compact_without_provider_usage_does_not_use_heuristic_fallback() {
    assert!(!should_compact_now(None, 1_048_576, 100));
}

#[test]
fn auto_compact_requires_compressible_messages() {
    assert!(!should_compact_now(Some(900_000), 1_048_576, 4));
}
```

- [x] **Step 2: Run the runtime tests and verify RED**

Run:

```bash
cargo test -p runtime auto_compact_
```

Expected: compilation fails because the old function has a different signature and still accepts raw token fields.

- [x] **Step 3: Replace Main compact state with `last_total_tokens`**

Use:

```rust
let mut last_total_tokens: Option<u64> = None;
```

After a successful provider response:

```rust
*self.last_total_tokens = Some(
    resp.usage
        .total_tokens
        .map(u64::from)
        .unwrap_or_else(|| {
            u64::from(resp.usage.input_tokens) + u64::from(resp.usage.output_tokens)
        }),
);
```

`needs_compaction` calls only `needs_compaction_total`. A successful compact sets `last_total_tokens = None`. Remove the one-compact-per-Run gate so a new provider response in the same Run can trigger another compact.

Preserve the existing uncommitted change that removes the duplicate threshold check inside the compact pipeline.

- [x] **Step 4: Run runtime tests and verify GREEN**

Run:

```bash
cargo test -p runtime auto_compact_
```

Expected: all matching tests pass.

### Task 4: Use Latest Total in Sub Runtime

**Files:**
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Test: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

- [x] **Step 1: Write a failing Sub compact state test**

Add a focused unit test proving:

```rust
assert!(!sub_needs_compaction(None, 1_048_576));
assert!(sub_needs_compaction(Some(900_000), 1_048_576));
```

- [x] **Step 2: Run the Sub test and verify RED**

Run:

```bash
cargo test -p runtime sub_needs_compaction
```

Expected: compilation fails because the helper/state does not exist.

- [x] **Step 3: Replace Sub raw compact counters**

Store `last_total_tokens: Option<u64>`, update it from normalized provider usage, compare it through `needs_compaction_total`, and reset it to `None` after compact. Keep raw input/output/cache fields only inside `StepTokenUsage` and cost/audit records.

- [x] **Step 4: Run the Sub test and verify GREEN**

Run:

```bash
cargo test -p runtime sub_needs_compaction
```

Expected: the Sub decision tests pass.

### Task 5: Cross-Layer Verification

**Files:**
- Verify only; no recent-tail files are modified for behavior.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --all
```

- [x] **Step 2: Run layer tests**

Run:

```bash
cargo test -p provider
cargo test -p context
cargo test -p runtime
```

Expected: all tests pass.

- [ ] **Step 3: Run compile and lint gates**

Run:

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both commands exit successfully with no warnings.

- [x] **Step 4: Verify scope**

Run:

```bash
git diff --check
git diff --name-only
```

Expected: recent-tail selection logic and Session Step schema are unchanged; only token normalization, compact decision/reset, tests, and aligned design/plan documents appear.

### Verification Notes

- Targeted `rustfmt --check` passed for every modified Rust file. Repository-wide
  `cargo fmt --all -- --check` remains blocked by pre-existing formatting differences
  outside this change.
- `cargo check --workspace` passed.
- Strict `cargo clippy --workspace --all-targets -- -D warnings` is blocked only by
  the pre-existing uncommitted `compact_summary.rs` change leaving `system_prompt`
  unused. Re-running with only `unused-variables` allowed passed, proving there are
  no additional clippy failures from this implementation.
- `cargo test -p provider`, `cargo test -p context`, and `cargo test -p runtime`
  passed after the final code changes.
