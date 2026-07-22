# EnterWorktree Empty Path and Base Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make blank `EnterWorktree.path` derive the target from `branch`, add an optional `base` whose blank/default value is `main`, return an accurate error for Primary checkouts, and preserve the attempted target in failed TUI headers.

**Architecture:** Tools owns string-input normalization and schema guidance; Project owns the `main` default, Git reference forwarding, repository/worktree invariants, and domain errors. TUI reads successful structured data first and falls back to the original invocation input on failure.

**Tech Stack:** Rust 2024, serde/serde_json, async-trait, git worktree CLI adapter, ratatui, Cargo tests.

---

### Task 1: Project enter contract, base default, and precise error

**Files:**
- Modify: `agent/features/project/src/domain/types.rs`
- Modify: `agent/features/project/src/domain/state.rs`
- Modify: `agent/features/project/src/domain/service.rs`
- Modify: `agent/features/project/src/domain/git.rs`
- Test: `agent/features/project/src/domain/state_tests.rs`
- Compatibility call sites: `agent/features/tools/src/domain/test_support.rs`
- Compatibility call sites: `agent/features/tools/src/adapters/worktree.rs`

- [ ] **Step 1: Write failing Project tests**

Extend the Project fake to record base values as well as paths:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAddCall {
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
}

pub added: Mutex<Vec<WorktreeAddCall>>,

fn worktree_add(
    &self,
    repo_root: &Path,
    path: &Path,
    branch: &str,
    base: &str,
) -> Result<(), GitOperationError> {
    self.added.lock().unwrap().push(WorktreeAddCall {
        repo_root: repo_root.to_path_buf(),
        path: path.to_path_buf(),
        branch: branch.to_string(),
        base: base.to_string(),
    });
    Ok(())
}
```

Add focused tests to `state_tests.rs`:

```rust
#[test]
fn empty_path_is_treated_as_missing_and_derives_from_branch() {
    let root = unique_temp_dir("empty_path");
    let state = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(root.join(".git").display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Primary,
    );

    assert_eq!(
        resolve_worktree_path(
            &state,
            Some(PathBuf::new()),
            Some("fix/example"),
        )
        .unwrap(),
        root.join(".worktrees/fix-example")
    );
}

#[test]
fn worktree_base_defaults_blank_values_and_preserves_explicit_value() {
    assert_eq!(worktree_base(None), "main");
    assert_eq!(worktree_base(Some("")), "main");
    assert_eq!(worktree_base(Some("   ")), "main");
    assert_eq!(worktree_base(Some("release/v1")), "release/v1");
}

#[test]
fn enter_primary_target_returns_not_linked_and_keeps_state() {
    let root = unique_temp_dir("primary_target");
    let common = root.join(".git");
    let mut state = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Primary,
    );
    let before = snapshot(&state);
    let mut git = FakeGit::default();
    git.toplevel.insert(root.clone(), root.clone());
    git.common_dir.insert(root.clone(), common);

    assert_eq!(
        enter(&mut state, &git, Some(root.clone()), None, None),
        Err(WorkspaceError::NotLinkedWorktree { path: root })
    );
    assert_eq!(snapshot(&state), before);
}
```

- [ ] **Step 2: Run Project tests and verify RED**

Run:

```bash
cargo test -p project --lib domain::state::tests::empty_path_is_treated_as_missing_and_derives_from_branch
```

Expected: compilation/test failure because `enter` has no `base` parameter and `NotLinkedWorktree` does not exist.

- [ ] **Step 3: Implement the minimal Project behavior**

Change the public control contract and domain rule signature:

```rust
fn enter(
    &self,
    path: Option<PathBuf>,
    branch: Option<String>,
    base: Option<String>,
) -> Result<WorkspaceFrame, WorkspaceError>;
```

Normalize path and base once in `state.rs`:

```rust
fn optional_non_empty_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|value| !value.as_os_str().is_empty())
}

fn worktree_base(base: Option<&str>) -> &str {
    base.filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_WORKTREE_BASE)
}
```

Resolve the target from `optional_non_empty_path(path)` and pass
`worktree_base(base.as_deref())` to `git.worktree_add`. Add:

```rust
NotLinkedWorktree { path: PathBuf },
```

with the Chinese display message:

```rust
"路径 {} 是当前仓库的 primary checkout，不是 linked worktree"
```

Use it only when repository identity matches but the probed kind is not `Linked`. Keep
`RepoMismatch` for a different git common dir. Thread `base` through
`WorkspaceService`. Update the Tools fake and adapter call sites to pass `None` until Task 2
publishes the Tool field.

- [ ] **Step 4: Run Project tests and verify GREEN**

Run:

```bash
cargo test -p project --lib domain::state::tests
```

Expected: all Project state tests pass.

- [ ] **Step 5: Commit Project behavior**

```bash
git add agent/features/project agent/features/tools/src/domain/test_support.rs agent/features/tools/src/adapters/worktree.rs
git commit -m "fix(project): normalize EnterWorktree path and base (#1297)"
```

### Task 2: Tools input normalization and base schema

**Files:**
- Modify: `agent/features/tools/src/domain/types/enter_worktree.rs`
- Modify: `agent/features/tools/src/adapters/worktree.rs`
- Test: `agent/features/tools/src/adapters/worktree.rs`

- [ ] **Step 1: Write failing schema and real-git adapter tests**

Extend the schema test:

```rust
assert_eq!(schema["properties"]["base"]["type"], "string");
assert!(schema["properties"]["path"]["description"]
    .as_str()
    .unwrap()
    .contains("禁止传空字符串"));
assert!(schema["properties"]["base"]["description"]
    .as_str()
    .unwrap()
    .contains("默认 main"));
```

Add a real-git regression test:

```rust
#[tokio::test]
async fn enter_worktree_blank_path_and_base_use_branch_path_and_main() {
    let tmp = tempfile::tempdir().unwrap();
    init_main_repo(tmp.path());
    let ctx = build_ctx(tmp.path().to_path_buf());
    let expected = tmp
        .path()
        .canonicalize()
        .unwrap()
        .join(".worktrees/fix-example");

    let result = enter_tool(&ctx)
        .call(
            serde_json::json!({
                "branch": "fix/example",
                "path": "",
                "base": ""
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "{}", result.text);
    let data = result.data.expect("successful enter returns data");
    assert_eq!(data.branch, "fix/example");
    assert_eq!(data.workspace_root, expected);
}
```

Add a real-git explicit-base test that first creates and commits `release-base`, returns to
`main`, calls `EnterWorktree` with a unique branch and `"base":"release-base"`, then verifies
the new worktree HEAD equals the `release-base` commit.

- [ ] **Step 2: Run Tools tests and verify RED**

Run:

```bash
cargo test -p tools --lib adapters::worktree::tests::enter_worktree_blank_path_and_base_use_branch_path_and_main
```

Expected: failure because `base` is not in `EnterWorktreeInput`, and blank path still reaches
Project as `Some(PathBuf(""))`.

- [ ] **Step 3: Implement the minimal Tools behavior**

Add the field:

```rust
/// 可选：创建新 worktree 的起点引用。省略、空串或纯空白时默认 main；进入已有 worktree 时忽略
pub base: Option<String>,
```

Strengthen `path` guidance:

```rust
/// 可选：worktree 根目录路径（绝对或相对路径）。无 path 时必须省略本字段，禁止传空字符串；系统从 branch 推导 .worktrees/<安全分支名>
```

Normalize only Tool string syntax before calling Project:

```rust
let path = args
    .path
    .filter(|value| !value.trim().is_empty())
    .map(PathBuf::from);
self.control.enter(path, args.branch, args.base)
```

Do not substitute `"main"` in Tools; Project remains the default-value owner.

- [ ] **Step 4: Run Tools tests and verify GREEN**

Run:

```bash
cargo test -p tools --lib adapters::worktree::tests
```

Expected: all EnterWorktree/ExitWorktree adapter tests pass.

- [ ] **Step 5: Commit Tools behavior**

```bash
git add agent/features/tools
git commit -m "feat(tools): add EnterWorktree base input (#1297)"
```

### Task 3: TUI failed-call target display

**Files:**
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/worktree.rs`
- Test: `apps/cli/src/tui/render/output/tool_display/tests.rs`

- [ ] **Step 1: Write failing TUI tests**

Add tests through the production formatting entry:

```rust
#[test]
fn enter_worktree_failure_header_uses_input_branch() {
    let payload = ToolResultPayload::new(
        "Failed to enter worktree".to_string(),
        serde_json::Value::Null,
        true,
        0,
    );
    let (header, _) = format_tool_call(
        "EnterWorktree",
        r#"{"branch":"fix/example","path":""}"#,
        Some(&payload),
        None,
    );
    assert_eq!(line_to_string(&header), "Enter Worktree branch=fix/example");
}

#[test]
fn enter_worktree_failure_header_uses_path_when_branch_missing() {
    let payload = ToolResultPayload::new(
        "Failed to enter worktree".to_string(),
        serde_json::Value::Null,
        true,
        0,
    );
    let (header, _) = format_tool_call(
        "EnterWorktree",
        r#"{"path":".worktrees/existing"}"#,
        Some(&payload),
        None,
    );
    assert_eq!(
        line_to_string(&header),
        "Enter Worktree path=.worktrees/existing"
    );
}
```

Retain or add a success test proving structured result branch/root wins over requested input.

- [ ] **Step 2: Run CLI test and verify RED**

Run:

```bash
cargo test -p cli --bin aemeath tui::render::output::tool_display::tests::enter_worktree_failure_header_uses_input_branch
```

Expected: FAIL with actual header `Enter Worktree branch=(default)`.

- [ ] **Step 3: Implement one shared target helper**

In `worktree.rs`, add a private input helper returning a label/value pair:

```rust
fn input_target(input: &serde_json::Value) -> (&'static str, Option<&str>) {
    input
        .get("branch")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| ("branch", Some(value)))
        .or_else(|| {
            input
                .get("path")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| ("path", Some(value)))
        })
        .unwrap_or(("target", None))
}
```

Use the helper in both `format_header` and `format_header_line_with_result`. For a successful
typed result, render `branch=<actual>` plus `(<workspace_root>)`; otherwise render the
non-empty input branch/path, or only the display name when neither exists.

- [ ] **Step 4: Run CLI tests and verify GREEN**

Run:

```bash
cargo test -p cli --bin aemeath tui::render::output::tool_display::tests
```

Expected: all ToolDisplay tests pass.

- [ ] **Step 5: Commit TUI behavior**

```bash
git add apps/cli/src/tui/render/output/tool_display
git commit -m "fix(cli): show failed EnterWorktree target (#1297)"
```

### Task 4: Target documentation and issue gates

**Files:**
- Modify: `docs/design/02-modules/project/01-domain-model.md`
- Modify: `docs/design/02-modules/project/02-ports-and-adapters.md`
- Modify: `specs/tui-cli.md`

- [ ] **Step 1: Update Project target documentation**

Document that empty optional path derives from branch, optional base defaults to `main`, explicit
base is forwarded only during creation, and `NotLinkedWorktree` is distinct from
`RepoMismatch`.

- [ ] **Step 2: Update TUI instruction**

Under ToolDisplayEntry, add:

```markdown
- result-aware header 覆写 **MUST** 在结构化 result 缺失或解析失败时回退消费原始 input，
  **NEVER** 用伪默认值掩盖实际调用参数。
```

- [ ] **Step 3: Verify documentation consistency**

Run:

```bash
rg -n "base|NotLinkedWorktree|result-aware" \
  docs/design/02-modules/project/01-domain-model.md \
  docs/design/02-modules/project/02-ports-and-adapters.md \
  specs/tui-cli.md
git diff --check
```

Expected: each new contract appears in its owning document and `git diff --check` exits 0.

- [ ] **Step 4: Commit documentation**

```bash
git add docs/design/02-modules/project specs/tui-cli.md
git commit -m "docs: align EnterWorktree base and display contracts (#1297)"
```

### Task 5: Verification, review, issue update, and PR

**Files:**
- Verify all changed files
- Update: GitHub issue `#1297`

- [ ] **Step 1: Run focused crates**

```bash
cargo test -p project
cargo test -p tools
cargo test -p cli --bin aemeath tui::render::output::tool_display
```

Expected: all tests pass with zero failures.

- [ ] **Step 2: Run repository gates**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
bash .agents/hooks/check-architecture-guards.sh
```

Expected: all commands exit 0. If the repository defines a different aggregate guard entry,
use the registered command from `.agents/aemeath.json` and record it in the PR.

- [ ] **Step 3: Request code review**

Review `origin/main..HEAD` against Issue #1297 and this plan. Fix all Critical and Important
findings, then repeat the affected tests.

- [ ] **Step 4: Update Issue gates**

Edit Issue #1297 so completed development/document/testing checklist items are checked. Record
any genuinely inapplicable L4/L5 evidence with a verifiable reason.

- [ ] **Step 5: Commit review fixes and push**

```bash
git status --short
git push -u origin fix/1297-enter-worktree-empty-path
```

- [ ] **Step 6: Create PR**

Create a PR to `main` using `.github/pull_request_template.md`. Include Summary, `Refs #1297`,
Breaking change (`No`), exact test commands, document checks, and the session reproduction.
Do not merge; wait for user review.
