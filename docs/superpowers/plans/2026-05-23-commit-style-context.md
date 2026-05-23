# Commit Style Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add system prompt guidance that tells the LLM to analyze repository commit style before creating commits and, when appropriate, use an Aemeath co-author trailer containing the current provider/model.

**Architecture:** Keep commit-style behavior as prompt guidance only. Do not execute `git log` during session initialization. Extend `build_system_prompt_parts` to receive provider/model metadata from the already-created `LlmClient`, build one centralized commit guidance string, and append it to the dynamic system prompt for both TUI and no-TUI flows.

**Tech Stack:** Rust, Tokio async, existing `cli/src/prompt.rs` system prompt builder, existing `aemeath_llm::client::LlmClient::{provider_name, model_name}`.

---

## File Structure

- Modify `cli/src/prompt.rs`
  - Add `PromptContext` carrying `cwd`, `provider_name`, and `model_name`.
  - Add `build_commit_guidance(provider_name, model_name)`.
  - Change `build_system_prompt_parts` to accept `PromptContext` instead of only `cwd`.
  - Append commit guidance to the dynamic system prompt.
  - Add unit tests near the existing tests in the same file.
- Modify `cli/src/run_orchestration.rs`
  - Pass `client.provider_name()` and `client.model_name()` into `PromptContext` when building system prompt parts.
- Update `docs/feature/active.md`
  - Mark #44 as implemented/待确认 after code is done.
- Keep `docs/feature/specs/044-commit-style-context.md`
  - No code execution step should add automatic `git log` analysis.

---

### Task 1: Add PromptContext and commit guidance tests

**Files:**
- Modify: `cli/src/prompt.rs`

- [ ] **Step 1: Add failing tests for commit guidance**

Append these tests inside the existing `#[cfg(test)] mod tests` in `cli/src/prompt.rs`. If no tests module exists at the end of the file, create one at the file end.

```rust
#[test]
fn test_build_commit_guidance_includes_provider_model_trailer() {
    let guidance = build_commit_guidance(Some("zhipu"), Some("glm-5.1"));

    assert!(guidance.contains("# Commit Message Guidance"));
    assert!(guidance.contains("git log --format=%B --grep='Co-Authored-By'"));
    assert!(guidance.contains(
        "Co-Authored-By: Aemeath (zhipu/glm-5.1) <github:rushsinging/aemeath>"
    ));
    assert!(guidance.contains("Do not invent human co-authors"));
}

#[test]
fn test_build_commit_guidance_uses_unknown_fallback() {
    let guidance = build_commit_guidance(None, None);

    assert!(guidance.contains(
        "Co-Authored-By: Aemeath (unknown/unknown) <github:rushsinging/aemeath>"
    ));
}

#[test]
fn test_prompt_context_new_preserves_model_metadata() {
    let cwd = PathBuf::from("/tmp/example");
    let context = PromptContext::new(&cwd, Some("openrouter"), Some("anthropic/claude-sonnet-4"));

    assert_eq!(context.cwd, cwd);
    assert_eq!(context.provider_name.as_deref(), Some("openrouter"));
    assert_eq!(
        context.model_name.as_deref(),
        Some("anthropic/claude-sonnet-4")
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo test -p aemeath-cli prompt::tests::test_build_commit_guidance_includes_provider_model_trailer prompt::tests::test_build_commit_guidance_uses_unknown_fallback prompt::tests::test_prompt_context_new_preserves_model_metadata
```

Expected: FAIL because `PromptContext` and `build_commit_guidance` do not exist yet.

- [ ] **Step 3: Implement PromptContext and commit guidance**

In `cli/src/prompt.rs`, add this near `SystemPromptParts`:

```rust
#[derive(Debug, Clone)]
pub struct PromptContext {
    pub cwd: PathBuf,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
}

impl PromptContext {
    pub fn new(cwd: &PathBuf, provider_name: Option<&str>, model_name: Option<&str>) -> Self {
        Self {
            cwd: cwd.clone(),
            provider_name: provider_name.map(str::to_string),
            model_name: model_name.map(str::to_string),
        }
    }
}
```

Add this helper below `static_system_prompt_for_test`:

```rust
fn build_commit_guidance(provider_name: Option<&str>, model_name: Option<&str>) -> String {
    let provider = provider_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown");
    let model = model_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown");
    let trailer = format!(
        "Co-Authored-By: Aemeath ({provider}/{model}) <github:rushsinging/aemeath>"
    );

    format!(
        r#"# Commit Message Guidance
When creating a git commit message:
- First inspect this repository's recent commit history and infer its Commit Style Context.
- Prefer sampling commits that contain `Co-Authored-By`, for example: `git log --format=%B --grep='Co-Authored-By' -n 20`.
- If there are no useful co-author examples, sample recent ordinary commits with a small limit.
- Analyze title format, type/scope usage, body style, language, footer/trailer conventions, and whether AI co-author trailers are commonly used.
- Keep the final commit message consistent with this repository's existing style.
- Do not invent human co-authors.
- When an AI co-author trailer is appropriate, use exactly: `{trailer}`."#
    )
}
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo test -p aemeath-cli prompt::tests::test_build_commit_guidance_includes_provider_model_trailer prompt::tests::test_build_commit_guidance_uses_unknown_fallback prompt::tests::test_prompt_context_new_preserves_model_metadata
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
git add cli/src/prompt.rs
git commit -m "feat(#44): add commit guidance builder"
```

---

### Task 2: Inject commit guidance into system prompt

**Files:**
- Modify: `cli/src/prompt.rs`

- [ ] **Step 1: Add failing async test for dynamic prompt injection**

Add this test inside `cli/src/prompt.rs` tests module:

```rust
#[tokio::test]
async fn test_build_system_prompt_parts_includes_commit_guidance() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let cwd = temp.path().to_path_buf();
    let hook_runner = HookRunner::empty(cwd.display().to_string());
    let memory_config = MemoryConfig::default();
    let context = PromptContext::new(&cwd, Some("deepseek"), Some("deepseek-chat"));

    let parts = build_system_prompt_parts(&context, &hook_runner, &memory_config).await;

    assert!(parts.dynamic_part.contains("# Commit Message Guidance"));
    assert!(parts.dynamic_part.contains(
        "Co-Authored-By: Aemeath (deepseek/deepseek-chat) <github:rushsinging/aemeath>"
    ));
    assert!(!parts.dynamic_part.contains("Commit Style Context:"));
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo test -p aemeath-cli prompt::tests::test_build_system_prompt_parts_includes_commit_guidance
```

Expected: FAIL because `build_system_prompt_parts` still accepts `&PathBuf` and does not inject commit guidance.

- [ ] **Step 3: Change build_system_prompt_parts signature and dynamic prompt**

In `cli/src/prompt.rs`, replace the function signature:

```rust
pub async fn build_system_prompt_parts(
    cwd: &PathBuf,
    hook_runner: &HookRunner,
    memory_config: &MemoryConfig,
) -> SystemPromptParts {
    let cwd_str = cwd.to_string_lossy();
    let is_git = is_git_repo(cwd).await;
```

with:

```rust
pub async fn build_system_prompt_parts(
    context: &PromptContext,
    hook_runner: &HookRunner,
    memory_config: &MemoryConfig,
) -> SystemPromptParts {
    let cwd = &context.cwd;
    let cwd_str = cwd.to_string_lossy();
    let is_git = is_git_repo(cwd).await;
```

Then after current date is appended:

```rust
let date = current_date();
dynamic.push_str(&format!("# currentDate\nToday's date is {date}."));
```

insert:

```rust
dynamic.push_str("\n\n");
dynamic.push_str(&build_commit_guidance(
    context.provider_name.as_deref(),
    context.model_name.as_deref(),
));
```

- [ ] **Step 4: Run test and verify it passes**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo test -p aemeath-cli prompt::tests::test_build_system_prompt_parts_includes_commit_guidance
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
git add cli/src/prompt.rs
git commit -m "feat(#44): inject commit style guidance"
```

---

### Task 3: Pass provider/model from orchestration

**Files:**
- Modify: `cli/src/run_orchestration.rs`
- Modify: `cli/src/prompt.rs`

- [ ] **Step 1: Run cargo check and observe call-site failure**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo check -p aemeath-cli
```

Expected: FAIL at `run_orchestration.rs` because `build_system_prompt_parts` now expects `&PromptContext`.

- [ ] **Step 2: Import PromptContext**

In `cli/src/run_orchestration.rs`, replace:

```rust
use crate::prompt::build_system_prompt_parts;
```

with:

```rust
use crate::prompt::{build_system_prompt_parts, PromptContext};
```

- [ ] **Step 3: Pass provider/model from LlmClient**

In `cli/src/run_orchestration.rs`, replace:

```rust
let prompt_parts = build_system_prompt_parts(&cwd, &hook_runner, &prompt_memory_config).await;
```

with:

```rust
let prompt_context = PromptContext::new(
    &cwd,
    Some(client.provider_name()),
    Some(client.model_name()),
);
let prompt_parts =
    build_system_prompt_parts(&prompt_context, &hook_runner, &prompt_memory_config).await;
```

- [ ] **Step 4: Run cargo check**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo check -p aemeath-cli
```

Expected: PASS.

- [ ] **Step 5: Run prompt tests**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo test -p aemeath-cli prompt::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
git add cli/src/prompt.rs cli/src/run_orchestration.rs
git commit -m "feat(#44): pass model metadata to prompt"
```

---

### Task 4: Update feature tracker and verify

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update #44 status**

In `docs/feature/active.md`, change #44 table status from `设计中` to `待确认`, and in the #44 details section change:

```markdown
**状态**：设计中
```

into:

```markdown
**状态**：待确认
```

Append an implementation note under #44 details:

```markdown
**实现记录（2026-05-23）**：已在 system prompt dynamic context 中加入 Commit Message Guidance；该 guidance 要求 LLM 在创建 commit 前分析当前仓库历史 commit 风格，优先采样带 `Co-Authored-By` 的提交；AI 协作者 trailer 使用 `Co-Authored-By: Aemeath (<provider>/<model>) <github:rushsinging/aemeath>`，provider/model 来自当前 `LlmClient`。未在 session 初始化执行 git log，也未提前生成历史摘要。
```

- [ ] **Step 2: Run full verification**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
cargo fmt --check
cargo test -p aemeath-cli prompt::tests
cargo check -p aemeath-cli
git diff --check
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
```

Expected: all commands PASS.

- [ ] **Step 3: Commit docs update**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature44-commit-style
git add docs/feature/active.md
git commit -m "docs(#44): mark commit guidance implemented"
```

- [ ] **Step 4: Merge back to main**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
git merge --no-ff feature/44-commit-style-context
```

Expected: merge commit created on `main`.

- [ ] **Step 5: Verify on main**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
cargo test -p aemeath-cli prompt::tests
cargo check -p aemeath-cli
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
git status --short
```

Expected: tests/check/hooks pass and status is clean.

- [ ] **Step 6: Clean worktree**

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
git worktree remove .worktrees/feature44-commit-style
git branch -d feature/44-commit-style-context
```

Expected: worktree removed and branch deleted.

---

## Self-Review

- Spec coverage:
  - System prompt guidance: Task 2.
  - Provider/model in trailer: Task 1 and Task 3.
  - No session-initial git log: implemented by prompt-only guidance and tested by absence of history summary in Task 2.
  - TUI and REPL shared path: both use `run_orchestration.rs` prompt construction before branching into TUI/no-TUI.
  - Docs tracker update: Task 4.
- Placeholder scan: no TBD/TODO/implement-later placeholders remain.
- Type consistency: `PromptContext::new`, `build_commit_guidance`, and `build_system_prompt_parts(&PromptContext, ...)` are consistently named across tasks.
