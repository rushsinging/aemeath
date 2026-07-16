# Compact Summary Continuation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 auto-compact 丢失首条用户输入并弱化后续行动语义的问题，使摘要准确汇总多个用户输入、已完成工作、发现的问题和下一步，并明确会话应继续、等待用户还是已完成。

**Architecture:** 保持 recent tail 的现有切分位置不变，只修正“被 compact 移除的消息必须全部进入 summary 输入”这一不变量。摘要仍由 Context 生成并经 Runtime 的 active-summary system block 注入，不新增第二条状态通道；通过更严格的 schema 表达用户请求、工作事实与 continuation 状态。

**Tech Stack:** Rust、Context Management compact adapter、Runtime main loop、Cargo test。

---

### Task 1: 固化首条用户输入丢失与摘要 schema

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`
- Test: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [x] **Step 1: 修改现有 early-window 测试，要求被移除的首条消息进入摘要输入**

将 `test_messages_selected_for_precompact_memory_uses_same_early_window_as_compact` 的期望改为：

```rust
assert_eq!(
    selected_text,
    vec![
        "message-0",
        "message-1",
        "message-2",
        "message-3",
        "message-4",
        "message-5",
    ]
);
```

- [x] **Step 2: 新增 prompt schema 回归测试**

```rust
#[test]
fn compact_prompt_preserves_user_requests_and_continuation_state() {
    assert!(COMPACT_PROMPT.contains("## User Requests"));
    assert!(COMPACT_PROMPT.contains("## Work Completed"));
    assert!(COMPACT_PROMPT.contains("## Problems / Findings"));
    assert!(COMPACT_PROMPT.contains("## Next Action"));
    assert!(COMPACT_PROMPT.contains("## Continuation Status"));
    assert!(COMPACT_PROMPT.contains("later corrections supersede"));
    assert!(COMPACT_PROMPT.contains("NEVER upgrade"));
}
```

- [x] **Step 3: 新增多个用户输入保真测试**

```rust
#[test]
fn compact_request_contains_all_user_inputs_in_order() {
    let request = build_compact_request(
        &[
            Message::user("看看 issue 850"),
            Message::user("只分析，不实现"),
            Message::user("按 segment 汇总"),
        ],
        100_000,
    );
    let text = request[0].text_content();
    let inspect = text.find("看看 issue 850").unwrap();
    let no_implementation = text.find("只分析，不实现").unwrap();
    let by_segment = text.find("按 segment 汇总").unwrap();
    assert!(inspect < no_implementation);
    assert!(no_implementation < by_segment);
}
```

- [x] **Step 4: 运行定向测试并确认 RED**

Run:

```bash
cargo test -p context compact_summary_tests -- --nocapture
```

Expected: early-window 断言和新 schema 断言失败；失败原因是首两条消息未进入 summary 输入且 prompt 尚无新字段。

### Task 2: 修正 summary 覆盖范围和 schema

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Test: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [x] **Step 1: 保持 recent tail 不变，令 summary 覆盖所有被移除消息**

将两处：

```rust
let early_messages = &messages[window.head_protect..window.split_point];
```

改为：

```rust
let early_messages = &messages[..window.split_point];
```

并将 `messages_selected_for_precompact_memory` 改为同一范围：

```rust
compact_window(messages.len())
    .map(|window| messages[..window.split_point].to_vec())
    .unwrap_or_default()
```

`split_point`、`keep_recent` 与 recent tail 的 `messages[window.split_point..]` 保持不变。

- [x] **Step 2: 更新 `COMPACT_PROMPT` 的精确结构**

结构必须至少包含：

```text
## User Requests
## Goal
## Work Completed
## Problems / Findings
## Key Decisions
## Relevant Files
## Current State
## Next Action
## Continuation Status
```

规则必须明确：

```text
- Consolidate all user inputs in chronological order; later corrections supersede earlier instructions.
- Preserve the requested action level exactly. NEVER upgrade inspect/diagnose/design/review into implement/edit/commit.
- Continuation Status must be exactly one of Continue, Waiting for User, or Completed.
- If status is Continue, the agent must execute Next Action without waiting for a new user instruction.
```

- [x] **Step 3: 让本地 fallback 使用相同顶层字段**

`build_summary_text` 输出 `User Requests`、`Work Completed`、`Problems / Findings`、`Current State`、`Next Action`、`Continuation Status` 标题；无法可靠推断的字段明确写为 unknown，而不是发明事实。

- [x] **Step 4: 运行定向测试并确认 GREEN**

Run:

```bash
cargo test -p context compact_summary_tests -- --nocapture
```

Expected: 全部通过。

### Task 3: 更新设计真相与 issue 证据

**Files:**
- Modify: `docs/design/02-modules/context-management/02-compact.md`
- Modify: GitHub issue `#671`

- [x] **Step 1: 更新设计文档**

在 L5 Auto-compact 章节记录：

```text
- summary 输入 MUST 覆盖所有将从 active messages 移除的消息；不存在“既不保留也不总结”的 head gap。
- summary MUST 按顺序汇总用户请求，并以最后修正覆盖早先冲突要求。
- summary MUST 区分 Work Completed、Problems / Findings、Current State、Next Action。
- summary MUST 输出 Continue / Waiting for User / Completed 三态 continuation。
- recent tail 的切分与 summary 覆盖范围是两个独立概念。
```

- [x] **Step 2: 更新 #671 的开发前文档—代码差异**

记录 session `019f6be7-4a48-75a7-a0e7-3fba07d2c078` 的证据：

```text
首条用户输入“看看issue 850”位于 head-protect 区，既未进入 summary，也未进入 recent tail；
生成的摘要因此将 inspect 意图升级为 implement，compact 后模型误判无待执行指令。
```

### Task 4: 完整验证

**Files:**
- Verify: `agent/features/context/**`
- Verify: `agent/features/runtime/**`
- Verify: workspace formatting and diff

- [x] **Step 1: 格式化**

Run:

```bash
cargo fmt --all -- --check
```

Result: `cargo fmt -p context -- --check` 通过。`cargo fmt --all -- --check` 被当前分支范围外的既有 TUI / Runtime 格式差异阻断；遵守变更边界，未改动无关文件。

- [x] **Step 2: 运行 Context 测试**

Run:

```bash
cargo test -p context
```

Expected: 0 failed。

- [x] **Step 3: 运行 Runtime 测试，验证 active summary 消费链未回归**

Run:

```bash
cargo test -p runtime
```

Expected: 0 failed。

- [x] **Step 4: 静态检查**

Run:

```bash
cargo check --workspace
git diff --check
```

Result: `cargo check --workspace` 与 `git diff --check` 通过；microcompact 实验已按用户要求撤销。

- [x] **Step 5: 检查变更边界**

Run:

```bash
git status --short
git diff -- agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs \
  docs/design/02-modules/context-management/02-compact.md \
  docs/superpowers/plans/2026-07-17-compact-summary-continuation.md
```

Result: recent tail 的 `split_point` 和 `messages[window.split_point..]` 行为未改变；microcompact 实验不在最终 diff 中。
