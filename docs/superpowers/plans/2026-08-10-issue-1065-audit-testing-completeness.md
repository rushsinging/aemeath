# Issue #1065 Audit Testing Completeness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为父项 #857 建立可追溯的 Audit L0～L5 测试证据，补齐确定性、模块协作和跨层场景缺口，完成全部验证门禁并交付 #1065 独立 PR。

**Architecture:** 保持现有 Audit 六边形边界不变：局部纯策略测试归 owning module，worker/query 协作测试经窄 Port Fake，文件适配器契约使用真实临时目录，Runtime/Composition/CLI 分别验证相邻边界。行为—测试矩阵记录每个稳定行为单元的层级、证据和不适用理由，coverage 与 production reachability 独立验收。

**Tech Stack:** Rust 2021、Tokio、async-trait、serde/serde_json、tempfile、Cargo、cargo-clippy、cargo-llvm-cov 0.8.7、Bash/Python architecture guards、GitHub CLI。

---

## File responsibility map

- Create `agent/features/audit/src/application/ingest_tests.rs`: worker 配置、sender、worker 编排、失败、指标和 shutdown 的 L1/L2 确定性测试。
- Create `agent/features/audit/src/application/query_tests.rs`: range、cursor、decoder、filter、summary 等纯查询策略的 L1 测试。
- Modify `agent/features/audit/src/application.rs`: 仅以 `#[cfg(test)] #[path = ...]` 声明同层测试模块。
- Modify `agent/features/audit/tests/usage_worker_contract.rs`: 保留跨公开面契约，移除短 sleep 驱动和已归位到 L1/L2 的重复白盒断言。
- Modify `agent/features/audit/tests/usage_query_contract.rs`: 保留 adapter/query 公共契约，补 unknown schema、storage error 和端到端 worker→query 场景。
- Modify `agent/composition/tests/audit_worker_assembly.rs`: 验证 Composition bridge、生产目录、配置冻结和 canonical Session 分区。
- Modify `apps/cli/src/chat.rs`: 通过窄 shutdown Port 使 frontend drain 可用 Fake 精确测试，保持生产使用 `SessionAudit`。
- Create `apps/cli/src/chat_tests.rs`: frontend 成功/失败与 drain outcome 正交的相邻边界测试。
- Modify `docs/design/02-modules/audit/01-usage-storage.md`: 回写最终 Current、行为—测试矩阵、覆盖率和验收结论。
- Modify `docs/superpowers/specs/2026-08-10-issue-1065-audit-testing-completeness-design.md`: 将状态更新为已实施并链接最终证据。

### Task 1: Capture clean baseline and evidence inventory

**Files:**
- Read: `agent/features/audit/**`
- Read: `agent/features/runtime/src/application/model/{usage.rs,usage_tests.rs,invocation.rs,invocation_usage_tests.rs}`
- Read: `agent/features/runtime/src/application/run/{context_factory.rs,context_factory_tests.rs}`
- Read: `agent/composition/src/audit.rs`
- Read: `agent/composition/tests/audit_worker_assembly.rs`
- Read: `apps/cli/src/chat.rs`

- [ ] **Step 1: Run the narrow baseline tests once and preserve the first result**

Run:

```bash
cargo test -p audit --all-targets
cargo test -p runtime invocation_usage
cargo test -p runtime usage_tests
cargo test -p runtime session_and_sub_runs_share_the_factory_usage_sink
cargo test -p composition --test audit_worker_assembly
cargo test -p cli chat::tests::frontend_preserves_original_result_when_audit_drain_is_absent
```

Expected: each command exits 0. If any command fails, record its first output before diagnosis; do not hide it with a rerun.

- [ ] **Step 2: Inventory test names without changing source**

Run:

```bash
cargo test -p audit --all-targets -- --list
cargo test -p runtime -- --list | rg 'usage|session_and_sub_runs_share_the_factory_usage_sink'
cargo test -p composition --test audit_worker_assembly -- --list
```

Expected: a complete list of existing Audit and adjacent-boundary tests suitable for the matrix.

- [ ] **Step 3: Commit only if baseline evidence required a tracked note**

No commit is expected. Store command evidence for the PR and Issue comments rather than creating a temporary repository file.

### Task 2: Add L1 query policy tests

**Files:**
- Create: `agent/features/audit/src/application/query_tests.rs`
- Modify: `agent/features/audit/src/application.rs`
- Test: `agent/features/audit/src/application/query_tests.rs`

- [ ] **Step 1: Register the separate owning-module test file**

Append to `agent/features/audit/src/application.rs`:

```rust
#[cfg(test)]
#[path = "application/query_tests.rs"]
mod query_tests;
```

- [ ] **Step 2: Write failing boundary tests**

Create `query_tests.rs` importing the private helpers from `super::query`. Add focused tests proving:

```rust
#[test]
fn validate_query_accepts_half_open_range_and_clamps_limit() { /* from == record timestamp is valid; limit becomes 1_000 */ }

#[test]
fn validate_query_rejects_equal_or_reversed_range() { /* both cases return InvalidRange */ }

#[test]
fn cursor_round_trip_preserves_unicode_stream_and_fingerprint() { /* encode then decode exact values */ }

#[test]
fn decode_cursor_rejects_bad_version_hex_offset_and_empty_stream() { /* table of malformed cursor values */ }

#[test]
fn query_fingerprint_changes_for_each_filter_but_not_pagination() { /* each PL filter changes fingerprint; cursor/limit do not */ }

#[test]
fn decode_record_rejects_unterminated_corrupt_and_unknown_schema_lines() { /* each maps to exact stream/line warning */ }

#[test]
fn matches_uses_inclusive_start_and_exclusive_end_for_every_filter() { /* exact lower bound matches; upper bound does not */ }

#[test]
fn add_summary_accumulates_optional_tokens_without_cost_fields() { /* None contributes zero; multiple records accumulate */ }
```

Use fixed IDs and timestamps. Do not use wall clock or random data.

- [ ] **Step 3: Run the new tests and verify the intended first failure**

Run:

```bash
cargo test -p audit --lib application::query_tests -- --nocapture
```

Expected before completing fixtures: compilation/test failure identifying any incorrect assumption in the test. Preserve the first failure in working notes.

- [ ] **Step 4: Complete the minimal test fixtures without changing production behavior**

Use only current public/domain constructors and direct helper calls. If all documented behavior already passes, production code remains unchanged.

- [ ] **Step 5: Run query L1 and public query contracts**

Run:

```bash
cargo test -p audit --lib application::query_tests
cargo test -p audit --test usage_query_contract
```

Expected: all tests pass, no sleeps or environment dependencies.

- [ ] **Step 6: Commit L1 query evidence**

```bash
git add agent/features/audit/src/application.rs agent/features/audit/src/application/query_tests.rs
git commit -m "test(audit): #1065 补齐 Usage 查询局部边界"
```

### Task 3: Replace worker sleeps with deterministic L1/L2 tests

**Files:**
- Create: `agent/features/audit/src/application/ingest_tests.rs`
- Modify: `agent/features/audit/src/application.rs`
- Modify: `agent/features/audit/tests/usage_worker_contract.rs`
- Test: `agent/features/audit/src/application/ingest_tests.rs`
- Test: `agent/features/audit/tests/usage_worker_contract.rs`

- [ ] **Step 1: Register the separate worker test module**

Append to `agent/features/audit/src/application.rs`:

```rust
#[cfg(test)]
#[path = "application/ingest_tests.rs"]
mod ingest_tests;
```

- [ ] **Step 2: Write deterministic test doubles**

In `ingest_tests.rs`, define a `ControlledStore` that owns:

```rust
struct ControlledStore {
    calls: std::sync::Mutex<Vec<StoreCall>>,
    append_started: tokio::sync::Notify,
    allow_append: tokio::sync::Semaphore,
    fail_append: bool,
    fail_flush: bool,
}

enum StoreCall {
    Append { stream: String, terminated: bool },
    Flush { stream: String },
}
```

`append` records and notifies, then waits for one semaphore permit; tests release the permit explicitly. `flush` records and optionally fails. `read` and `list_streams` return `AppendLogError::Closed` because these methods are outside worker scope.

- [ ] **Step 3: Write failing L1/L2 tests**

Add:

```rust
#[test]
fn worker_config_enforces_capacity_floor_and_zero_timeout_default() { /* 0 => 1 and 5s */ }

#[tokio::test]
async fn full_queue_drops_immediately_while_first_append_is_blocked() { /* wait append_started, fill capacity, next try_record => QueueFull */ }

#[tokio::test]
async fn worker_calls_append_then_flush_in_fifo_order_and_drains() { /* release each append deterministically */ }

#[tokio::test]
async fn append_failure_skips_flush_counts_once_and_continues() { /* two records, first/each failure does not kill worker */ }

#[tokio::test]
async fn flush_failure_counts_once_and_continues() { /* append succeeds, flush fails, next record still completes */ }

#[tokio::test(start_paused = true)]
async fn shutdown_timeout_counts_exact_unconfirmed_records_and_is_idempotent() { /* advance virtual time, exact TimedOut value */ }

#[tokio::test]
async fn sender_rejects_after_shutdown_and_metrics_keep_conservation_equation() { /* accepted = completed + unconfirmed while dropped categories stay distinct */ }
```

- [ ] **Step 4: Run new worker tests and preserve the first failure**

Run:

```bash
cargo test -p audit --lib application::ingest_tests -- --nocapture
```

Expected: tests expose any mismatch in exact timeout or metrics semantics. Do not weaken exact assertions to `>=` unless the domain contract itself is amended.

- [ ] **Step 5: Make only the minimal root-cause implementation correction if tests reveal a contract violation**

Allowed production changes are restricted to `agent/features/audit/src/application/ingest.rs`. Preserve these invariants:

```rust
accepted_total == completed_total + drain_abandoned_total
```

at terminal shutdown, each accepted record increments `completed_total` exactly once unless explicitly abandoned, and append failure does not call flush.

- [ ] **Step 6: Remove sleep-driven duplicate tests from the public worker contract**

Keep public API tests for `Accepted`, `QueueFull`, `WorkerUnavailable`, `Drained`, `TimedOut`, and metrics getters. Replace `Duration::from_millis(100)`/`Duration::from_secs(1)` store sleeps with the event-driven `ControlledStore` pattern or delete only assertions now covered more strongly by the owning L2 tests. Do not delete unique public-contract behavior.

- [ ] **Step 7: Run all worker tests twice only as a stability check**

Run:

```bash
cargo test -p audit --lib application::ingest_tests
cargo test -p audit --test usage_worker_contract
cargo test -p audit --lib application::ingest_tests
cargo test -p audit --test usage_worker_contract
```

Expected: both independent passes succeed without timing-sensitive variation. The first pass remains the correctness evidence; the second is only a stability signal.

- [ ] **Step 8: Commit deterministic worker evidence**

```bash
git add agent/features/audit/src/application.rs agent/features/audit/src/application/ingest.rs agent/features/audit/src/application/ingest_tests.rs agent/features/audit/tests/usage_worker_contract.rs
git commit -m "test(audit): #1065 确定化 Usage worker 协作测试"
```

If `ingest.rs` was unchanged, omit it from `git add`.

### Task 4: Complete adapter/query contracts and Audit L4 journey

**Files:**
- Modify: `agent/features/audit/tests/usage_query_contract.rs`
- Modify: `agent/features/audit/tests/append_store_contract.rs` only if an uncovered adapter contract is confirmed
- Test: `agent/features/audit/tests/usage_query_contract.rs`

- [ ] **Step 1: Write a fake failing query store contract test**

Add a private `FailingQueryStore` implementing `UsageAppendStorePort`; `list_streams` and `read` can independently return `AppendLogError::Io`. Add:

```rust
#[tokio::test]
async fn query_maps_list_and_read_failures_to_storage_error() { /* both paths => UsageQueryError::Storage */ }
```

- [ ] **Step 2: Add unknown schema and precise warning evidence**

Append a newline-terminated envelope whose `schema_version` differs from `CURRENT_USAGE_SCHEMA_VERSION`, query it, and assert `CorruptLine` contains the exact stream and line number while valid neighboring records remain visible.

- [ ] **Step 3: Add the sender→worker→file→query scenario**

Add:

```rust
#[tokio::test]
async fn accepted_usage_drains_to_file_then_queries_and_summarizes() {
    // start real FileUsageAppendStore worker
    // send fixed UsageRecord through UsageSender
    // shutdown => Drained
    // query exact session/provider/model/time range
    // assert exact record and token summary
}
```

Do not manually encode this scenario's record; the worker owns envelope/framing.

- [ ] **Step 4: Run the scenario and contract tests**

Run:

```bash
cargo test -p audit --test append_store_contract
cargo test -p audit --test usage_query_contract
```

Expected: all adapter and L4 tests pass using unique temp directories.

- [ ] **Step 5: Commit adapter and L4 evidence**

```bash
git add agent/features/audit/tests/append_store_contract.rs agent/features/audit/tests/usage_query_contract.rs
git commit -m "test(audit): #1065 补齐存储查询契约与落盘场景"
```

Omit `append_store_contract.rs` if unchanged.

### Task 5: Strengthen Runtime and Composition adjacent-boundary evidence

**Files:**
- Modify: `agent/features/runtime/src/application/model/invocation_usage_tests.rs`
- Modify: `agent/features/runtime/src/application/run/context_factory_tests.rs` only if canonical Session identity is not already asserted
- Modify: `agent/composition/tests/audit_worker_assembly.rs`
- Test: same paths

- [ ] **Step 1: Add complete Runtime UsageRecord field assertions**

Extend the successful logical invocation test to assert exact values for:

```rust
recorded_at_unix_ms
session_id
run_id
run_step_id
model_invocation_id
provider
model
input_tokens
output_tokens
cache_write_tokens
cache_read_tokens
reasoning_tokens
```

Also assert a `Dropped(QueueFull)` outcome does not alter the returned invocation response.

- [ ] **Step 2: Add no-fact terminal variants at the closest callable boundary**

Where the current invocation test seam permits, assert failure/cancel/unreported usage never calls the sink. If failure and cancellation return before `record_successful_usage`, cite their existing invocation tests in the matrix instead of exposing a new test-only API.

- [ ] **Step 3: Verify Main/Sub share the same sink without duplicating state-machine tests**

Keep `session_and_sub_runs_share_the_factory_usage_sink` as the L2 assembly proof. Add exact canonical Session identity assertions only if the existing factory test does not already prove both contexts derive from the same session backing.

- [ ] **Step 4: Upgrade Composition's production worker scenario**

After shutdown, construct `usage_query_service(file_usage_append_store(SafeStorageRoot::open(agents_dir.join("audit"))))`, query the fixed session, and assert the exact record. This proves `agents_dir/audit/usage/<canonical-session>.jsonl` is not merely created but contains the complete PL fact.

- [ ] **Step 5: Run adjacent-boundary tests**

Run:

```bash
cargo test -p runtime invocation_usage
cargo test -p runtime session_and_sub_runs_share_the_factory_usage_sink
cargo test -p composition --test audit_worker_assembly
```

Expected: all pass; no test crosses crate-private internals.

- [ ] **Step 6: Commit cross-layer evidence**

```bash
git add agent/features/runtime/src/application/model/invocation_usage_tests.rs agent/features/runtime/src/application/run/context_factory_tests.rs agent/composition/tests/audit_worker_assembly.rs
git commit -m "test(audit): #1065 强化 Runtime 与 Composition 使用链路"
```

Omit unchanged files.

### Task 6: Make frontend drain testable without coupling CLI to Audit internals

**Files:**
- Modify: `apps/cli/src/chat.rs`
- Create: `apps/cli/src/chat_tests.rs`
- Test: `apps/cli/src/chat_tests.rs`

- [ ] **Step 1: Write tests against a narrow shutdown capability**

Define test fixtures in `chat_tests.rs`:

```rust
struct RecordingAuditShutdown {
    calls: std::sync::atomic::AtomicUsize,
    outcome: audit::UsageShutdownOutcome,
}
```

Add four tests:

```rust
#[tokio::test]
async fn frontend_success_drains_audit_once_and_preserves_success() { /* Drained */ }

#[tokio::test]
async fn frontend_failure_drains_audit_once_and_preserves_original_error() { /* Drained */ }

#[tokio::test]
async fn frontend_success_ignores_audit_timeout_outcome() { /* TimedOut must not become frontend error */ }

#[tokio::test]
async fn frontend_failure_ignores_audit_timeout_and_preserves_original_error() { /* original exact error remains */ }
```

- [ ] **Step 2: Run tests to verify the current concrete parameter blocks the fake**

Run:

```bash
cargo test -p cli chat_tests -- --nocapture
```

Expected: compile failure because `run_frontend_with_audit_drain` currently accepts only `Option<&composition::audit::SessionAudit>`.

- [ ] **Step 3: Introduce the minimal private shutdown Port**

In `chat.rs`, define:

```rust
#[async_trait::async_trait]
trait AuditShutdown: Sync {
    async fn shutdown(&self) -> audit::UsageShutdownOutcome;
}

#[async_trait::async_trait]
impl AuditShutdown for composition::audit::SessionAudit {
    async fn shutdown(&self) -> audit::UsageShutdownOutcome {
        composition::audit::SessionAudit::shutdown(self).await
    }
}
```

Change only the helper parameter to `Option<&dyn AuditShutdown>`. Production call sites continue passing `bootstrap.session_audit.as_ref().map(|value| value as &dyn AuditShutdown)`; no public API is added.

Replace the inline `#[cfg(test)] mod tests` with:

```rust
#[cfg(test)]
#[path = "chat_tests.rs"]
mod chat_tests;
```

Move all existing chat tests unchanged into `chat_tests.rs` and add the four drain tests.

- [ ] **Step 4: Run CLI tests and architecture organization guards**

Run:

```bash
cargo test -p cli chat_tests
.agents/hooks/check-no-inline-tests.sh
.agents/hooks/check-unit-tests.sh
```

Expected: all pass; no inline test module and no new public test-only surface.

- [ ] **Step 5: Commit frontend drain evidence**

```bash
git add apps/cli/src/chat.rs apps/cli/src/chat_tests.rs
git commit -m "test(cli): #1065 验证 Audit drain 不覆盖前端结果"
```

### Task 7: Build and publish the L0-L5 evidence matrix

**Files:**
- Modify: `docs/design/02-modules/audit/01-usage-storage.md`
- Modify: `docs/superpowers/specs/2026-08-10-issue-1065-audit-testing-completeness-design.md`

- [ ] **Step 1: Add the twelve-row behavior matrix**

For every stable behavior unit, add columns:

```text
行为/风险 | L0 | L1 | L2 | L3 | L4 | L5 | 最终测试/Guard 路径 | 结论
```

Use exact test names and paths. Mark L5 `N/A` with the reason that real file-system behavior is covered by adapter contracts and L4, while no network/PTY/install/platform-only semantic exists.

- [ ] **Step 2: Add issue-leaf traceability**

Map #927, #928, #929, #930, #931, #932 and #988 to the rows they delivered. Issue/PR identifiers are allowed in this implementation governance record but must not be inserted into code comments or stable architecture prose.

- [ ] **Step 3: Classify findings**

Add four explicit lists:

```text
文档错误
实现缺口
测试缺口
过期测试
```

Each list states what was found, how it was corrected, or `无` with evidence. Do not leave placeholders.

- [ ] **Step 4: Record preliminary verification and coverage slots only after commands run**

Do not enter predicted numbers. Add actual line/region/function totals and percentages from Task 9, changed-lines evidence when available, and exact command outcomes.

- [ ] **Step 5: Mark the approved design implemented**

Change the spec status from `已批准，待实施` to `已实施，证据见 ...` only after all behavior tests and documentation are present.

- [ ] **Step 6: Check documentation consistency**

Run:

```bash
rg -n 'TBD|TODO|待补|待定|CostTracker|cost_history\.json' \
  docs/design/02-modules/audit \
  docs/superpowers/specs/2026-08-10-issue-1065-audit-testing-completeness-design.md
git diff --check
```

Expected: no placeholders; retired names appear only where explicitly documenting the retirement guard.

- [ ] **Step 7: Commit the matrix**

```bash
git add docs/design/02-modules/audit/01-usage-storage.md docs/superpowers/specs/2026-08-10-issue-1065-audit-testing-completeness-design.md
git commit -m "docs(audit): #1065 回写测试矩阵与验收证据"
```

### Task 8: Run formatting, targeted, production, and architecture gates

**Files:**
- Verify only

- [ ] **Step 1: Format with rustfmt, then prove no drift**

Run:

```bash
cargo fmt --all
cargo fmt --all --check
```

Expected: second command exits 0. If rustfmt changes files, inspect and commit them with the owning task rather than a standalone formatting commit.

- [ ] **Step 2: Run all Audit-adjacent targeted tests**

Run:

```bash
cargo test -p audit --all-targets
cargo test -p runtime invocation_usage
cargo test -p runtime usage_tests
cargo test -p runtime session_and_sub_runs_share_the_factory_usage_sink
cargo test -p composition --test audit_worker_assembly
cargo test -p cli chat_tests
```

Expected: all pass on first final-gate run.

- [ ] **Step 3: Run production-only checks**

Run:

```bash
cargo check -p audit --lib
cargo check -p runtime --lib
cargo check -p composition --lib
cargo check -p cli --bin aemeath
cargo clippy -p audit --lib -- -D warnings
cargo clippy -p runtime --lib -- -D warnings
cargo clippy -p composition --lib -- -D warnings
cargo clippy -p cli --bin aemeath -- -D warnings
```

Expected: all pass, proving production reachability independent from tests.

- [ ] **Step 4: Run all-targets clippy for affected crates**

Run:

```bash
cargo clippy -p audit -p runtime -p composition -p cli --all-targets -- -D warnings
```

Expected: exits 0.

- [ ] **Step 5: Run focused guards before the full registry**

Run:

```bash
.agents/hooks/check-provider-usage-capability.sh
.agents/hooks/check-cost-tracker-retirement.sh
.agents/hooks/check-cost-tracker-retirement-tests.sh
.agents/hooks/check-production-reachability.sh
.agents/hooks/check-no-inline-tests.sh
.agents/hooks/check-unit-tests.sh
```

Expected: all focused guards pass.

- [ ] **Step 6: Run the full architecture guard registry**

Run:

```bash
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh --full
```

Expected: `All full architecture guards passed.`

### Task 9: Run workspace and coverage gates

**Files:**
- Verify only
- Modify after evidence: `docs/design/02-modules/audit/01-usage-storage.md`

- [ ] **Step 1: Verify coverage tool prerequisite**

Run:

```bash
cargo llvm-cov --version
```

Expected: `cargo-llvm-cov 0.8.7`. A missing/wrong version is a concrete environment blocker; do not silently install a global binary without user approval.

- [ ] **Step 2: Run the repository coverage gate**

Run:

```bash
scripts/coverage.sh
```

Expected: repository coverage thresholds pass. Preserve the first output.

- [ ] **Step 3: Produce an Audit-focused JSON report without replacing the repository gate**

Run:

```bash
mkdir -p target/coverage-evidence
cargo llvm-cov -p audit --all-targets --json --summary-only \
  --output-path target/coverage-evidence/audit-summary.json
jq '.data[0].files[] | select(.filename | contains("agent/features/audit/")) | {filename,summary}' \
  target/coverage-evidence/audit-summary.json
```

Expected: actual line, region and function values for Audit source files. `target/**` remains untracked.

- [ ] **Step 4: Record changed-lines signal**

Run:

```bash
git diff --unified=0 origin/main...HEAD -- 'agent/features/audit/**/*.rs' 'agent/features/runtime/**/*.rs' 'agent/composition/**/*.rs' 'apps/cli/**/*.rs'
```

Correlate changed executable lines with the focused coverage report. If the repository has no automated changed-lines threshold, document that fact and explain each uncovered changed production branch manually.

- [ ] **Step 5: Run workspace tests and clippy**

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both pass on first final-gate run.

- [ ] **Step 6: Insert actual verification evidence and commit**

Update the Audit design with exact command outcomes and measured coverage values, then run:

```bash
git add docs/design/02-modules/audit/01-usage-storage.md
git commit -m "docs(audit): #1065 记录覆盖率与最终门禁"
```

### Task 10: Review, push, update Issues, and create PR

**Files:**
- Review all changed files
- GitHub Issue #1065
- GitHub Issue #857
- New PR targeting `main`

- [ ] **Step 1: Review diff scope and repository cleanliness**

Run:

```bash
git status --short
git diff --check
git diff --stat origin/main...HEAD
git log --oneline origin/main..HEAD
```

Expected: only planned test, necessary production seam, design and plan files are changed; working tree is clean.

- [ ] **Step 2: Run verification-before-completion review**

Invoke `superpowers:verification-before-completion`, then rerun any command it requires. Do not claim completion from stale output.

- [ ] **Step 3: Request code review**

Invoke `superpowers:requesting-code-review`. Review findings by severity; correct every blocking or in-scope issue and rerun affected gates.

- [ ] **Step 4: Push the branch**

Run:

```bash
git push -u origin test/1065-audit-testing-completeness
```

Expected: branch push succeeds.

- [ ] **Step 5: Update #1065 with the complete evidence**

Use `gh issue edit 1065 --body-file <generated-temp-file>` to preserve the original goal and checklist while adding:

- the twelve-row L0–L5 matrix or a stable document link plus exact summary;
- first-failure record;
- test/implementation/document/obsolete-test classification;
- command and coverage evidence;
- remaining gaps (`无` if fully closed);
- final parent-level acceptance conclusion.

Check every completed checkbox only when evidence exists.

- [ ] **Step 6: Update #857 parent acceptance section**

Use `gh issue edit 857 --body-file <generated-temp-file>` to preserve original scope and add the #1065 conclusion, PR link placeholder if needed, and statement that L5 is not applicable with rationale. Do not close #857 before the PR is merged.

- [ ] **Step 7: Create the PR**

Run:

```bash
gh pr create \
  --base main \
  --head test/1065-audit-testing-completeness \
  --title "test(audit): #1065 完成 Usage 审计测试验收" \
  --body-file <generated-pr-body>
```

The body must include `Closes #1065` and `Refs #857`, changed behavior, matrix summary, exact verification commands/results, coverage values, first-failure status, and remaining risk.

- [ ] **Step 8: Verify remote PR checks and Issue links**

Run:

```bash
gh pr view --json number,url,state,isDraft,mergeable,statusCheckRollup,closingIssuesReferences
gh issue view 1065 --json state,body,url
gh issue view 857 --json state,body,url
```

Expected: PR open and mergeable or a concrete reported blocker; #1065 is linked for closure on merge; #857 remains open and references the acceptance result.

- [ ] **Step 9: Report full project progress**

Include:

- #857 completed direct children / total and percentage;
- #1065 phase completion percentage based on the ten tasks above;
- milestone closed/total and percentage queried fresh from GitHub;
- PR URL and checks state;
- all verification results and any remaining blocker.
