# Compact Continuation Checkpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 #1558 建立分层、可规范化、可恢复验证的 compact continuation checkpoint，使 Agent 在 compact 与 Session Resume 后能准确继续原任务，同时减少重复、过期动态事实和机械截断造成的语义损失。

**Architecture:** Context 继续独占 compact summary 的生成、规范化和 canonical `ActiveCompactMarker.summary` 提交；新增 Context-owned checkpoint schema/normalizer，统一约束 LLM map/reduce/refresh 与 deterministic fallback 的最终出口。Runtime 仍只消费 `ContextWindow`，Task 状态继续经 #1537 的 `task_context` 在 summary 定稿后追加，不新增 Runtime summary backing、第二存储或触发阈值策略。

**Tech Stack:** Rust、async_trait、Context Management、CanonicalSession dataset/AtomicBlob、Runtime ContextPort、shell architecture guards、Cargo test/clippy/fmt。

**Issue:** [#1558](https://github.com/rushsinging/aemeath/issues/1558)，原生父项 #864，能力父项 #547，milestone `v0.1.0 — Context Engineering + 架构重构`。

---

## 文件职责与变更地图

- Create: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`
  - 只负责 checkpoint 九分区、三态 continuation、可选 `Current Task State` companion 的拆分、解析、规范化、预算降级和稳定渲染；不执行 LLM、Session IO 或 Runtime 编排。
- Create: `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`
  - 覆盖 schema、唯一 Resume Cursor、动态事实重验证、重复抑制和语义优先级降级。
- Modify: `agent/features/context/src/domain/compact.rs`
  - 注册并在 Context crate 内导出 continuation checkpoint 能力。
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
  - 更新初次与 refresh prompt；让 LLM、map/reduce/refresh、previous summary 与 fallback 都汇聚到同一 normalizer；删除机械 tail-authoritative 截断。
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`
  - 覆盖 prompt 契约、oversized previous summary、模型输出规范化和 fallback。
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
  - 在 canonical commit 前校验最终 checkpoint，并保持 #1537 Task append 与 CAS 协议不变。
- Modify: `agent/features/context/tests/canonical_session_repository.rs`
  - 覆盖连续 compact、过时 working set 淘汰、Task 单一追加和 marker/revision 不变量。
- Modify: `agent/features/context/tests/dataset_session_reader.rs`
  - 覆盖 dataset 落盘后 Resume 的 checkpoint 保真。
- Modify: `agent/features/context/tests/application_service_contract.rs`
  - 覆盖 active summary 作为唯一 Context-owned system block 出站。
- Modify: `agent/features/runtime/src/application/loop_engine/llm_strategy_tests.rs`
  - 覆盖 Runtime 逐字消费 Context system block，不解析或重写 checkpoint。
- Create: `.agents/hooks/check-compact-continuation-checkpoint.sh`
  - 禁止恢复机械 previous-summary head/tail authoritative 截断和 Runtime summary backing/拼装路径。
- Modify: `.agents/hooks/check-architecture-guards.sh`
  - 注册新 Guard。
- Modify: `.agents/architecture-guard-registry.json`
  - 登记 Guard、owner、范围和能力分类。
- Modify: `docs/design/02-modules/context-management/02-compact.md`
  - 固化 Current schema、预算优先级、质量降级和 Issue 边界。
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
  - 记录 Guard 的正反例与边界。

## 测试层级

- **L1 Domain**：checkpoint parser/normalizer/render 的确定性和不变量。
- **L2 Adapter/Application**：LLM/fallback/refresh 输出汇聚、Task append、canonical commit。
- **L3 Contract**：Context `active_summary → system_blocks` 与 Runtime `ContextWindow → InvocationContext` 相邻层契约。
- **L4 Scenario/Resume**：dataset 落盘、连续 compact、Resume 后语义等价。
- **Guard**：静态阻止机械截断和第二 summary owner 回归。
- **L5**：不新增真实 provider E2E；LLM 输出质量用确定性 fake fixtures 覆盖，避免非确定性和外部成本。

---

### Task 1: 建立 continuation checkpoint 领域契约

**Files:**
- Create: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`
- Create: `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`
- Modify: `agent/features/context/src/domain/compact.rs`

- [ ] **Step 1: 写九分区与三态解析红测**

在 `continuation_checkpoint_tests.rs` 创建完整 fixture，并断言固定顺序、单一 Resume Cursor 与 typed status：

```rust
use super::*;

const COMPLETE_CHECKPOINT: &str = r#"## Immutable Constraints
- NEVER merge PR #1541.

## Current Objective
- Continue the content-stream migration without widening scope.

## Committed Facts
- Commit `5e42c9aa` passed Runtime and CLI tests.

## Uncommitted Working Set
- Runtime and SDK are migrated; TUI remains.

## Open Decisions / Risks
- `chat_result.rs` compatibility ownership requires inspection.

## Resume Cursor
- Worktree: `.worktrees/feat-945-1502-control-terminal-convergence`
- Branch: `feat/945-1502-control-terminal-convergence`
- Current task: migrate TUI consumers
- Next action: inspect all legacy Token/Thinking consumers.
- Prohibited: do not merge PR #1541.

## Required Revalidation
- Recheck worktree status, branch HEAD, PR state, and CI before mutation.

## Archived Milestones
- ToolCall split completed in `5e42c9aa`.

## Continuation Status
Continue — TUI consumption remains."#;

#[test]
fn parses_complete_checkpoint_with_one_resume_cursor() {
    let checkpoint = ContinuationCheckpoint::parse(COMPLETE_CHECKPOINT)
        .expect("checkpoint must parse");

    assert_eq!(checkpoint.status(), ContinuationStatus::Continue);
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert_eq!(checkpoint.render(), COMPLETE_CHECKPOINT);
}
```

- [ ] **Step 2: 写拒绝缺失、重复和非法状态的红测**

覆盖以下 fixture：缺少 `Immutable Constraints`；出现两个 `## Resume Cursor`；`Resume Cursor` 有两个 `Next action`；`Continuation Status` 为 `In Progress`。断言分别返回 `MissingSection`、`DuplicateSection`、`InvalidResumeCursor`、`InvalidStatus`，且错误包含中文用户可诊断消息：

```rust
#[test]
fn rejects_duplicate_resume_cursor_and_ambiguous_next_action() {
    let error = ContinuationCheckpoint::parse(&format!(
        "{COMPLETE_CHECKPOINT}\n\n## Resume Cursor\n- Next action: second action"
    ))
    .expect_err("duplicate cursor must fail");

    assert!(matches!(error, CheckpointError::DuplicateSection { .. }));
}
```

- [ ] **Step 3: 运行定向测试确认 RED**

Run:

```bash
cargo test -p context continuation_checkpoint_tests -- --nocapture
```

Expected: FAIL，原因是 `ContinuationCheckpoint`、`ContinuationStatus` 和 `CheckpointError` 尚未定义。

- [ ] **Step 4: 实现最小领域模型与稳定 parser/render**

在 `continuation_checkpoint.rs` 定义：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationStatus {
    Continue,
    WaitingForUser,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeCursor {
    lines: Vec<String>,
    next_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationCheckpoint {
    immutable_constraints: Vec<String>,
    current_objective: Vec<String>,
    committed_facts: Vec<String>,
    uncommitted_working_set: Vec<String>,
    open_decisions_and_risks: Vec<String>,
    resume_cursor: ResumeCursor,
    required_revalidation: Vec<String>,
    archived_milestones: Vec<String>,
    continuation_reason: String,
    status: ContinuationStatus,
}
```

实现严格标题 parser：每个标题恰好一次、顺序固定；`Resume Cursor` 恰好一个 `Next action:`；`Continuation Status` 首词只允许三态。`render()` 始终输出固定顺序和单空行，不保留重复标题或不稳定空白。

- [ ] **Step 5: 运行定向测试确认 GREEN**

Run:

```bash
cargo test -p context continuation_checkpoint_tests -- --nocapture
```

Expected: PASS，0 failed。

- [ ] **Step 6: 提交领域契约**

```bash
git add agent/features/context/src/domain/compact.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs
git commit -m "feat(context): #1558 define continuation checkpoint"
```

---

### Task 2: 实现语义优先级预算与重复抑制

**Files:**
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`

- [ ] **Step 1: 写 oversize、动态事实和重复事实红测**

构造 30K 字符 checkpoint：噪声分别放在头部 `Archived Milestones` 和中部 `Committed Facts`，关键约束/目标/cursor/revalidation/status 分布在不同位置。调用 `normalize_to_budget(source, 4_000)` 后断言：

```rust
#[test]
fn budget_normalization_preserves_continuation_critical_sections_by_semantics() {
    let normalized = ContinuationCheckpoint::parse(&oversized_checkpoint())
        .unwrap()
        .normalize_to_budget(4_000)
        .unwrap();
    let rendered = normalized.render();

    assert!(rendered.contains("NEVER merge PR #1541"));
    assert!(rendered.contains("Continue the content-stream migration"));
    assert!(rendered.contains("Next action: inspect all legacy consumers"));
    assert!(rendered.contains("Recheck worktree status"));
    assert!(rendered.contains("Continue —"));
    assert!(!rendered.contains("ARCHIVE-NOISE-099"));
}
```

再构造同一事实同时位于 `Committed Facts`、`Uncommitted Working Set`、`Archived Milestones` 的 fixture；断言稳定引用仅保留在 `Committed Facts`，working set 不复制已提交事实，archive 只保留一行 commit 引用。

- [ ] **Step 2: 写动态状态归类红测**

输入 `PR #1541 is OPEN and CI is green`、`worktree is clean`、`origin branch matches HEAD` 等当前态到 `Committed Facts`，规范化后断言它们移动到 `Required Revalidation` 并改为 recheck 指令；历史提交和已完成测试仍留在 `Committed Facts`。

- [ ] **Step 3: 运行定向测试确认 RED**

Run:

```bash
cargo test -p context continuation_checkpoint_tests::budget_normalization \
  continuation_checkpoint_tests::dynamic_state -- --nocapture
```

Expected: FAIL，原因是 `normalize_to_budget` 和归一化规则尚未实现。

- [ ] **Step 4: 实现固定语义退化顺序**

实现 `normalize_to_budget(max_tokens)`，复用 `crate::domain::token_budget::estimate_tokens`，按以下顺序删减，禁止字符串头尾切片：

1. 去除跨分区完全重复行；
2. 把动态当前态转为 `Required Revalidation`；
3. `Archived Milestones` 每项压为稳定引用一行；
4. 删除最旧且已有 archive 引用的 `Committed Facts` 细节；
5. 删除不再属于当前目标的 working-set 细节；
6. 收紧 `Open Decisions / Risks` 的解释，只保留风险与待决动作；
7. 永不删除 `Immutable Constraints`、`Current Objective`、`Resume Cursor`、`Required Revalidation` 和 `Continuation Status`。

若保护分区本身已超过预算，返回：

```rust
CheckpointError::ProtectedSectionsExceedBudget {
    estimated_tokens,
    budget,
}
```

不得静默截断。

- [ ] **Step 5: 运行定向测试确认 GREEN**

Run:

```bash
cargo test -p context continuation_checkpoint_tests -- --nocapture
```

Expected: PASS，0 failed。

- [ ] **Step 6: 提交预算策略**

```bash
git add agent/features/context/src/domain/compact/continuation_checkpoint.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs
git commit -m "feat(context): #1558 prioritize checkpoint semantics"
```

---

### Task 3: 统一 LLM prompt、refresh 与最终输出规范化

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [ ] **Step 1: 写新 prompt contract 红测**

替换旧的 `compact_prompt_preserves_user_requests_and_continuation_state`，断言 `COMPACT_PROMPT` 和 `COMPACT_REFRESH_PROMPT` 都包含九个固定标题，并包含：

```text
- Put GitHub/CI/remote branch/worktree current state under Required Revalidation.
- State each detailed fact in one authoritative section; do not duplicate it elsewhere.
- Resume Cursor MUST contain exactly one Next action and explicit prohibited actions.
- Archived Milestones contain one-line stable references, not process transcripts.
```

同时断言 prompt 不再要求“More detail is better than less”或“use the budget fully”。

- [ ] **Step 2: 写不合规模型输出规范化红测**

建立 fake generator，依次返回：缺少 `Required Revalidation`；重复 `Resume Cursor`；旧九章节格式；动态 PR 状态错误放在 committed facts。调用 `compact_messages_with_llm`，断言最终 `CompactResult.summary` 总是可由 `ContinuationCheckpoint::parse` 解析，或 generator 错误时显式进入 deterministic fallback；不得原样提交不合规文本。

- [ ] **Step 3: 运行 adapter 测试确认 RED**

Run:

```bash
cargo test -p context compact_summary_tests -- --nocapture
```

Expected: FAIL，旧 prompt 无新分区，`parse_compact_response` 后未做 checkpoint 规范化。

- [ ] **Step 4: 更新初次与 refresh prompt**

将两个 prompt 统一为领域 schema；初次 prompt强调保真和单一权威分区，refresh prompt强调按语义优先级压缩。两者都要求 `<summary>` 内完整 checkpoint，且明确 dynamic state 只作为 revalidation requirement。

- [ ] **Step 5: 增加最终出口 normalizer**

在 adapter 中新增窄函数：

```rust
fn normalize_generated_checkpoint(
    summary: &str,
    budget: usize,
) -> Result<String, String> {
    let (checkpoint_text, task_state) = split_checkpoint_and_task_state(summary);
    let checkpoint = ContinuationCheckpoint::parse(checkpoint_text)
        .map_err(|error| error.to_string())?;
    let normalized = checkpoint
        .normalize_to_budget(budget)
        .map_err(|error| error.to_string())?
        .render();
    Ok(append_optional_task_state(normalized, task_state))
}
```

`split_checkpoint_and_task_state` 只接纳 previous summary 末尾由 #1537 追加的单一 `## Current Task State` companion；它不属于九分区 LLM schema，先与 checkpoint 分离，再在 normalizer 出口原样追加。`llm_generate` 仍只负责提取 `<summary>`；单次、map reduce 和 refresh 的最终结果在返回 `CompactResult` 前统一调用 normalizer。中间 map shard 可使用同一 schema，但只在最终 reduce 后强制完整 contract，避免 shard 因缺少全局 cursor 被误判。

- [ ] **Step 6: 运行 adapter 测试确认 GREEN**

Run:

```bash
cargo test -p context compact_summary_tests -- --nocapture
```

Expected: PASS，0 failed。

- [ ] **Step 7: 提交 LLM 路径**

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs
git commit -m "feat(context): #1558 normalize compact checkpoints"
```

---

### Task 4: 替换 deterministic fallback 和 previous-summary 机械截断

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [ ] **Step 1: 把现有 tail 截断测试改为语义保真红测**

删除以下旧期望：

```rust
assert!(text.contains("<previous_summary_tail>"));
```

改为构造关键内容分别位于 previous summary 头、中、尾的 checkpoint，并断言 `build_compact_request` 传入的是 `<previous_checkpoint>` 规范化结果，包含约束、目标、唯一 Next action、revalidation 和 status；不包含 `<previous_summary_tail>`、`older head truncated` 或 archive 噪声。

- [ ] **Step 2: 写 fallback 九分区与未验证事实红测**

`build_summary_text` 的输出必须可 parse；assistant 文本和 ToolUse 只能进入 `Open Decisions / Risks` 或带 `unverified` 标记的 working set；不得进入 `Committed Facts`。最新 unresolved user request 形成 `Current Objective` 和唯一 Next action；assistant 等待/完成报告保持 `Waiting for User`。previous summary 若带 #1537 `## Current Task State` companion，fallback 必须先分离该 companion，避免把旧 Task 快照送入 LLM 或混入 checkpoint，最终由当前请求的 typed `task_context` 重新追加最新状态。

- [ ] **Step 3: 运行定向测试确认 RED**

Run:

```bash
cargo test -p context compact_summary_tests::compact_request_caps_oversized_previous_summary \
  compact_summary_tests::fallback_summary -- --nocapture
```

Expected: FAIL，现有实现机械 `slice_tail` 且 fallback 使用旧章节。

- [ ] **Step 4: 实现 previous checkpoint 有界化**

删除 `slice_tail` 的 authoritative previous-summary 路径。若 previous summary 可解析，调用 `normalize_to_budget(previous_summary_budget)` 后完整嵌入 `<previous_checkpoint>`；若是 legacy summary，则先通过 `ContinuationCheckpoint::from_legacy_summary` 保守迁移：旧 `User Requests/Goal` → Current Objective，旧 constraints/decisions → Immutable Constraints 或 Risks，旧 Next Action → Resume Cursor，GitHub/CI 当前态 → Required Revalidation。无法可靠归类的文本进入 Risks 并标 `unverified legacy summary`。

- [ ] **Step 5: 用领域 builder 重写 fallback**

`build_summary_text` 先构造 typed `ContinuationCheckpoint`，再 `normalize_to_budget` 和 render；禁止用大 `format!` 复制第二份 schema。previous checkpoint 通过 typed merge 合并，latest user correction 覆盖旧 objective，旧 working set 仅在仍与 current objective 相关时保留。

- [ ] **Step 6: 运行 Context adapter 测试确认 GREEN**

Run:

```bash
cargo test -p context compact_summary_tests -- --nocapture
```

Expected: PASS，且无 mechanical tail-authoritative 测试残留。

- [ ] **Step 7: 提交 fallback 修复**

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs
git commit -m "fix(context): #1558 preserve checkpoint semantics on fallback"
```

---

### Task 5: 固化 canonical commit、连续 compact 与 Task 单一真相

**Files:**
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/tests/canonical_session_repository.rs`

- [ ] **Step 1: 写连续 compact 场景红测**

在 repository test 中让 generator 第一次输出完整 checkpoint，第二次输出包含第一次 previous checkpoint 和新用户修正的 checkpoint。断言第二次 committed marker：

- 保留 immutable constraint；
- current objective 为后续修正；
- resume cursor 只有一个 Next action；
- 旧 working set 被 archive 稳定引用取代；
- `source_revision` 等于第二次 compact 前 revision；
- marker 仍只有一个。

- [ ] **Step 2: 扩展 #1537 Task append 红测**

令 LLM checkpoint 的 `Uncommitted Working Set` 含一句普通任务描述，再传 typed `task_context`。断言 committed summary 恰好一个 `## Current Task State`，其内容只来自 `task_context`；checkpoint normalizer 不创建第二个 Task backing，map/reduce/refresh 输入也不包含 task snapshot。

- [ ] **Step 3: 写非法最终 checkpoint 拒绝提交红测**

让 generator 返回不可规范化且保护分区超预算的 checkpoint；断言 canonical revision、marker、persisted generation 均不变，并返回 typed `ContextPortError::Compact`，不静默 commit 部分结果。

- [ ] **Step 4: 运行 repository 测试确认 RED**

Run:

```bash
cargo test -p context --test canonical_session_repository continuation_checkpoint -- --nocapture
cargo test -p context --test canonical_session_repository commit_compaction_appends_task_context -- --nocapture
```

Expected: 新测试 FAIL；现有 commit 前没有显式 checkpoint validation。

- [ ] **Step 5: 在 commit 边界增加最终校验且保持协议不变**

`compact_visible_messages` 返回 normalized summary；`commit_compaction` 和 `commit_manual_compaction` 在 `append_task_context` 前调用同一 `validate_final_checkpoint`。校验器必须先分离可选 `Current Task State` companion，只校验九分区 checkpoint；提交时丢弃 previous companion，并且只追加当前请求的 typed `task_context`，保证该段最多一份且不会陈旧累加。不得改变 mutation gate、`source_revision`、CAS、marker `start_at`、revision increment 或 AtomicBlob commit plan。

- [ ] **Step 6: 运行 repository 测试确认 GREEN**

Run:

```bash
cargo test -p context --test canonical_session_repository -- --nocapture
```

Expected: PASS，0 failed。

- [ ] **Step 7: 提交 canonical 场景**

```bash
git add agent/features/context/src/adapters/canonical_session.rs \
  agent/features/context/tests/canonical_session_repository.rs
git commit -m "feat(context): #1558 commit resumable checkpoints"
```

---

### Task 6: 证明 Session Resume 与 Runtime 消费链等价

**Files:**
- Modify: `agent/features/context/tests/dataset_session_reader.rs`
- Modify: `agent/features/context/tests/application_service_contract.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/llm_strategy_tests.rs`

- [ ] **Step 1: 写 dataset Resume 红测**

构造含完整 checkpoint marker 的 canonical session，使用真实 dataset writer 保存，再由 reader `load_for_resume`。断言恢复后的 `active_summary` 字节等于 committed checkpoint + 单一 Current Task State；compact 前隐藏 steps 不回到可见 messages，`source_revision` 和 `start_at` 保持。

- [ ] **Step 2: 写 Context system block 契约红测**

扩展 `application_service_contract.rs`：active summary 恰好生成一个名为 `active_summary` 的 cacheable system block；内容逐字等于 checkpoint，不另行提取 Resume Cursor、不删除 Required Revalidation、不生成第二 summary block。

- [ ] **Step 3: 写 Runtime 逐字消费红测**

在 `llm_strategy_tests.rs` 构造包含 checkpoint 的 `ContextWindow.system_blocks`，调用 `extract_invocation_context`，断言对应 `RequestSystemBlock` 内容逐字相同且只有一个；Runtime 不 parse、不重排、不追加 continuation 内容。

- [ ] **Step 4: 运行跨层红测**

Run:

```bash
cargo test -p context --test dataset_session_reader continuation_checkpoint -- --nocapture
cargo test -p context --test application_service_contract active_summary -- --nocapture
cargo test -p runtime llm_strategy_tests::continuation_checkpoint --lib -- --nocapture
```

Expected: 新测试在当前 fixture/helper 缺失处 FAIL。

- [ ] **Step 5: 补齐最小 fixture 与真实消费链，不修改 Runtime 生产逻辑**

只在 Context reader/assembler 存在字段丢失时修改生产代码；若现有生产逻辑已满足，保持 production 不变，以契约测试锁定。禁止为测试新增 Runtime checkpoint parser 或 public API。

- [ ] **Step 6: 运行跨层测试确认 GREEN**

Run:

```bash
cargo test -p context --test dataset_session_reader -- --nocapture
cargo test -p context --test application_service_contract -- --nocapture
cargo test -p runtime llm_strategy_tests --lib -- --nocapture
```

Expected: PASS，0 failed。

- [ ] **Step 7: 提交 Resume/Runtime 契约**

```bash
git add agent/features/context/tests/dataset_session_reader.rs \
  agent/features/context/tests/application_service_contract.rs \
  agent/features/runtime/src/application/loop_engine/llm_strategy_tests.rs
git commit -m "test(context): #1558 preserve checkpoint across resume"
```

---

### Task 7: 增加架构 Guard 防止机械截断和第二 owner 回归

**Files:**
- Create: `.agents/hooks/check-compact-continuation-checkpoint.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 先写 Guard 自测的 deliberate negative probes**

Guard 支持 `AEMEATH_GUARD_ROOT` fixture root。创建临时 fixture 依次放入：

1. Context adapter 使用 `slice_tail(previous_summary, ...)` 或 `slice_head` 作为 authoritative previous summary；
2. Runtime production 定义 `active_summary` 字段；
3. Runtime production 拼接 `## Resume Cursor` 或 `## Continuation Status`；
4. 合法 Context domain normalizer 和 Runtime 只读 `ContextWindow`。

自测脚本断言前三项失败、第四项通过。

- [ ] **Step 2: 运行 Guard 自测确认 RED**

Run:

```bash
bash .agents/hooks/check-compact-continuation-checkpoint.sh --self-test
```

Expected: FAIL，因为 Guard 尚未实现或未覆盖 negative probe。

- [ ] **Step 3: 实现窄 Guard**

Guard 仅扫描生产路径并报告具体文件/行：

```text
agent/features/context/src/adapters/compact_summary.rs
agent/features/context/src/domain/compact/**
agent/features/runtime/src/**
```

允许 Context normalizer 内部按 section/line 重写；禁止对 whole previous summary 使用 head/tail slicing。Runtime 继续沿用 `check-shared-run-loop.sh` 的 summary owner 边界，并新增禁止 checkpoint 标题拼装。测试、docs 和 fixture 不计入生产违规。

- [ ] **Step 4: 注册 Guard 并更新架构文档**

在 registry 使用稳定 capability 名 `context.compact-continuation-checkpoint`，owner 为 Context；在总 Guard 中执行；文档记录禁止项、允许项、fixture 自测和与现有 shared-run-loop Guard 的关系。

- [ ] **Step 5: 运行 Guard 正反验证**

Run:

```bash
bash .agents/hooks/check-compact-continuation-checkpoint.sh --self-test
bash .agents/hooks/check-compact-continuation-checkpoint.sh
bash .agents/hooks/check-architecture-guards.sh
```

Expected: self-test 的 deliberate failures 被正确识别；真实仓库 PASS。

- [ ] **Step 6: 提交 Guard**

```bash
git add .agents/hooks/check-compact-continuation-checkpoint.sh \
  .agents/hooks/check-architecture-guards.sh \
  .agents/architecture-guard-registry.json \
  docs/design/03-engineering/01-architecture-guards.md
git commit -m "test(guards): #1558 protect compact checkpoints"
```

---

### Task 8: 更新 Compact 设计真相和 Issue checklist

**Files:**
- Modify: `docs/design/02-modules/context-management/02-compact.md`
- Modify: GitHub Issue `#1558`

- [ ] **Step 1: 更新 Summary 保真度不变量**

将 Current schema 改为九分区 continuation checkpoint，明确：

```text
Immutable Constraints → Current Objective → Committed Facts →
Uncommitted Working Set → Open Decisions / Risks → Resume Cursor →
Required Revalidation → Archived Milestones → Continuation Status
```

记录“同一事实单 owner”“唯一 Next action”“动态状态必须 revalidate”“Task State 为 checkpoint 外 typed append”。

- [ ] **Step 2: 更新预算与 fallback 退化顺序**

写明语义 section 级退化顺序，不再允许 whole-summary head/tail authoritative 截断；保护分区超过预算时显式失败/quality downgrade，不伪装为完整 LLM summary。

- [ ] **Step 3: 更新边界映射**

在与 #547 的映射中加入 #1558，并明确：#1537 只负责 typed Task append；#1162 仍是 Future 增量树；#1558 不改变 trigger/context-window/provider error 策略。

- [ ] **Step 4: 对照 Issue checklist 记录证据**

用 `gh issue comment 1558 --repo rushsinging/aemeath --body-file <evidence>` 回写每个 L1-L4、Guard、文档和验证项的测试命令/结果；只勾选有证据的 checkbox，未完成项保留并说明 blocker。

- [ ] **Step 5: 提交设计文档**

```bash
git add docs/design/02-modules/context-management/02-compact.md
git commit -m "docs(context): #1558 define checkpoint quality contract"
```

---

### Task 9: 全量验证、死路径审计和交付准备

**Files:**
- Verify: `agent/features/context/**`
- Verify: `agent/features/runtime/**`
- Verify: `.agents/hooks/**`
- Verify: `docs/design/**`

- [ ] **Step 1: 运行格式化检查**

```bash
cargo fmt --all -- --check
```

Expected: PASS。若发现范围外既有差异，记录文件、`origin/main` 复现命令和结果，不手工格式化无关代码。

- [ ] **Step 2: 运行 Context 全量测试**

```bash
cargo test -p context
```

Expected: 0 failed；ignored 性能基线保持 ignored。

- [ ] **Step 3: 运行 Runtime 全量测试**

```bash
cargo test -p runtime --lib
```

Expected: 0 failed。

- [ ] **Step 4: 运行 clippy**

```bash
cargo clippy -p context -p runtime --all-targets -- -D warnings
```

Expected: PASS，无新增 warning。

- [ ] **Step 5: 运行完整架构守卫**

```bash
bash .agents/hooks/check-architecture-guards.sh --full
```

Expected: PASS，包含 `context.compact-continuation-checkpoint`。

- [ ] **Step 6: 检查 diff 与退役路径**

```bash
git diff --check
git grep -n -E 'previous_summary_tail|older head truncated|slice_(head|tail)\(.*previous_summary' -- \
  agent/features/context/src agent/features/runtime/src
git grep -n -E '## (Resume Cursor|Continuation Status)' -- \
  agent/features/runtime/src ':!**/*tests.rs'
git status --short
```

Expected: production 无旧机械截断；Runtime production 不拼装 checkpoint；status 仅含计划内文件。

- [ ] **Step 7: 对照 #1558 全部 checklist**

逐项把测试证据映射到 Issue checkbox。无法完成或不适用的项必须在 Issue comment 记录理由、影响和后续，不得静默跳过。

- [ ] **Step 8: 拉取最新 main 并重跑受影响门禁**

```bash
git pull origin main
cargo test -p context
cargo test -p runtime --lib
bash .agents/hooks/check-architecture-guards.sh --full
```

Expected: 无冲突，全部 PASS。

- [ ] **Step 9: 提交任何仅由验证产生的计划内修正**

若验证没有产生修正，跳过本步；否则：

```bash
git add <only-planned-files>
git commit -m "fix(context): #1558 close checkpoint verification gaps"
```

- [ ] **Step 10: 推送并创建 PR（不得合并）**

```bash
git push -u origin plan/1558-compact-continuation-checkpoint
gh pr create --repo rushsinging/aemeath \
  --base main \
  --head plan/1558-compact-continuation-checkpoint \
  --title "feat(context): improve compact continuation checkpoints" \
  --body-file /tmp/pr-1558.md
```

PR body 必须包含 `Closes #1558`、Summary、Breaking change=`No`、完整 Test plan、Guard 证据和 out-of-scope。创建后查询 base/head、Draft、mergeable、checks；**NEVER merge**，等待用户 review。

---

## 计划自审结果

- **Issue 覆盖**：#1558 的 L1-L4、Guard、文档、验证 checklist 均有对应任务。
- **边界一致**：不改 auto-compact 触发时机，不实施 #1162，不复制 #1537 Task backing；`Current Task State` 被定义为九分区 checkpoint 外的 typed companion，连续 compact 时由当前请求替换，不送入 LLM；不新增 Runtime summary owner。
- **类型一致**：统一使用 `ContinuationCheckpoint`、`ContinuationStatus`、`ResumeCursor`、`CheckpointError` 和 `normalize_to_budget`。
- **TDD 顺序**：每项核心逻辑均先红测、再最小实现、再绿测；跨 Context → Runtime 与持久化 → Resume 每层都有相邻证据。
- **无占位符**：placeholder scan 已执行；实现步骤、命令和预期结果均明确。
