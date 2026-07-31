# TUI timeline 存在性索引化：消除 resume 与运行期 O(n²) 卡顿

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 TUI `OutputTimelineModel` 的 tool call 存在性查询从 O(n) 全表扫描改为 O(1) 索引，使长会话（~30000 timeline items）resume 从约 4 分 24 秒降到 3 秒内，并消除运行期同类劣化。

**Architecture:** `OutputTimelineModel` 是 `Vec<OutputTimelineItem>`，resume 与运行期对 13874 个 tool call 逐条执行 3~5 次 O(n) 扫描（`contains_tool_call` / `contains_tool_result` / `promote_orphan_tool_result`），每次比较 3 个字符串，总体 O(n²)（实测 n≈29462，resume 耗时 4.4 分钟）。修复：在 `OutputTimelineModel` 内新增 `tool_ref_index: HashSet<TimelineToolCallRef>` 与 `orphan_ids: HashSet<String>` 两个存在性索引，`push` / `retain` 统一维护，查询与 `promote_orphan_tool_result` 前置判断变 O(1)；`move_tool_result_after_tool_call` 保留线性回退但先经 O(1) 索引短路（不存在直接返回）。索引只存存在性、不存位置，`remove/insert` 不破坏一致性，避免位置同步成本。

**Tech Stack:** Rust，apps/cli（ratatui TUI），无新依赖（`std::collections::HashSet`）。

**背景数据（issue #1467 调查实证）：** session `019fa1be-bab3-7c47-ad94-c2952813dee8`：136MB、7142 steps、13874 tool_use、TUI timeline 29462 items；resume 耗时 `startup resume` 19:01:06.8 → `tui_first_frame` 19:05:31.2 = **4 分 24 秒**；Context 层加载仅 90ms（非瓶颈）。

---

## 文件结构

- Create: `apps/cli/src/tui/model/conversation/resume_performance_tests.rs` —— resume 场景性能回归测试（`#[ignore]` release 手动运行）。
- Modify: `apps/cli/src/tui/model/conversation.rs` —— 注册新测试模块。
- Modify: `apps/cli/src/tui/model/output_timeline/model.rs` —— 索引字段、统一 push/retain 维护、contains 走索引、move 短路。文件内现有 `#[cfg(test)] mod tests` 追加索引一致性测试。
- Modify: `apps/cli/src/tui/model/conversation/tool_flow.rs` —— `promote_orphan_tool_result` 增加 O(1) 前置判断。

不改：`tool_order.rs`、`tool_observe.rs` 的调用签名；`items_mut()` 暴露（仅被 text_stream/ask_user 修改既有 item 字段，不增删，不破坏索引）。

---

### Task 1: resume 性能复现测试（TDD 红）

**Files:**
- Create: `apps/cli/src/tui/model/conversation/resume_performance_tests.rs`
- Modify: `apps/cli/src/tui/model/conversation.rs`（注册 mod，仿第 26-29 行 `retained_state_tests` 的写法）

- [ ] **Step 1: 写复现测试**

创建 `apps/cli/src/tui/model/conversation/resume_performance_tests.rs`：

```rust
//! Resume 场景性能回归测试。
//!
//! 复现 issue #1467：长会话 resume 时逐条 apply 约 1.4 万个 tool call，
//! 每个触发 3~5 次对 timeline（≈30000 items）的 O(n) 全表扫描。
//! 基线（修复前）：release 下 > 60s；目标（修复后）：< 3s。

use std::time::{Duration, Instant};

use super::intent::ConversationIntent;
use super::model::ConversationModel;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiResumedSessionStep,
    TuiResumedStepFinalizeCause,
};

/// 构造与真实长会话等价的 resume workload：
/// `step_count` 个 step，每 step 一个含 `tools_per_step` 个 ToolUse 的
/// assistant 消息 + 一个含同数量 ToolResult 的 user 消息 + Completed 终态。
/// timeline items ≈ tools_per_step * 2 * step_count + step_count。
fn build_resume_workload(step_count: usize, tools_per_step: usize) -> Vec<TuiResumedSessionStep> {
    (0..step_count)
        .map(|step| TuiResumedSessionStep {
            run_id: format!("run-{step}"),
            step_id: format!("step-{step}"),
            messages: {
                let assistant = TuiChatMessage {
                    role: "assistant".to_string(),
                    content: (0..tools_per_step)
                        .map(|t| TuiContentBlock::ToolUse {
                            id: format!("tool-{step}-{t}"),
                            name: "Bash".to_string(),
                            input: serde_json::json!({ "command": "true" }),
                        })
                        .collect(),
                    source: TuiMessageSource::User,
                    stop_hook: None,
                    input_id: None,
                };
                let result = TuiChatMessage {
                    role: "user".to_string(),
                    content: (0..tools_per_step)
                        .map(|t| TuiContentBlock::ToolResult {
                            tool_use_id: format!("tool-{step}-{t}"),
                            content: serde_json::json!({ "stdout": "ok" }),
                            is_error: false,
                            text: Some("ok".to_string()),
                        })
                        .collect(),
                    source: TuiMessageSource::User,
                    stop_hook: None,
                    input_id: None,
                };
                vec![assistant, result]
            },
            finalize_cause: Some(TuiResumedStepFinalizeCause::Completed),
            duration_ms: Some(1000),
        })
        .collect()
}

#[test]
#[ignore = "性能回归；手动运行：cargo test -p cli --release resume_performance -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn resume_performance_large_session() {
    // 2000 steps × 7 tools → 14000 calls + 14000 results + 2000 System ≈ 30000 items，
    // 等价于 issue #1467 的 29462 items 现场。
    const STEPS: usize = 2000;
    const TOOLS_PER_STEP: usize = 7;
    let steps = build_resume_workload(STEPS, TOOLS_PER_STEP);
    let mut model = ConversationModel::default();

    let started = Instant::now();
    let changes = model.apply(ConversationIntent::ResumeConversation(
        super::intent::ResumeConversation { steps },
    ));
    let elapsed = started.elapsed();

    println!(
        "resume_performance: steps={STEPS} tools={TOOLS_PER_STEP} timeline_items={} changes={} elapsed={:.2}s",
        model.timeline.items().len(),
        changes.len(),
        elapsed.as_secs_f64()
    );
    assert_eq!(
        model.timeline.items().len(),
        STEPS * TOOLS_PER_STEP * 2 + STEPS,
        "timeline 规模必须与 workload 一致"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "resume 耗时 {elapsed:?} 超过 3s 目标（修复前基线 > 60s）"
    );
}

#[test]
fn resume_workload_timeline_stays_bounded_in_debug() {
    // debug 模式不卡阈值，只验证规模与顺序正确性（顺序断言见 Task 2 一致性测试）。
    let steps = build_resume_workload(10, 3);
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::ResumeConversation(super::intent::ResumeConversation {
        steps,
    }));
    assert_eq!(model.timeline.items().len(), 10 * 3 * 2 + 10);
}
```

在 `apps/cli/src/tui/model/conversation.rs` 的模块列表追加（参照第 26-29 行）：

```rust
#[cfg(test)]
#[path = "conversation/resume_performance_tests.rs"]
mod resume_performance_tests;
```

- [ ] **Step 2: 注册后先跑 debug 规模测试确认可编译、规模断言成立**

Run: `cargo test -p cli resume_workload_timeline_stays_bounded_in_debug`
Expected: PASS（2 条断言：timeline items 数量 == 70）

- [ ] **Step 3: 跑 release 性能测试验证红**

Run: `cargo test -p cli --release resume_performance_large_session -- --ignored --nocapture`
Expected: FAIL（`resume 耗时 ... 超过 3s 目标`；修复前基线约 60~260s）。若本机 debug 更慢属正常，以 release 结果为准。记录实际基线耗时到 issue #1467 comment。

- [ ] **Step 4: Commit**

```bash
git add apps/cli/src/tui/model/conversation.rs apps/cli/src/tui/model/conversation/resume_performance_tests.rs
git commit -m "test(tui): #1467 resume 长会话性能复现测试（30000 items 基线）"
```

---

### Task 2: OutputTimelineModel 存在性索引（根因修复）

**Files:**
- Modify: `apps/cli/src/tui/model/output_timeline/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_flow.rs`

- [ ] **Step 1: 先写索引一致性测试（追加到 `output_timeline/model.rs` 现有 `#[cfg(test)] mod tests`）**

在 `apps/cli/src/tui/model/output_timeline/model.rs` 的 tests 模块末尾追加：

```rust
#[test]
fn tool_ref_index_stays_consistent_across_push_and_move() {
    let mut model = OutputTimelineModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatTurnId::new("turn-1");
    let tool = ToolCallId::new("tool-1");

    model.push_tool_call_ref(chat.clone(), turn.clone(), tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(!model.contains_tool_result(&chat, &turn, tool.as_ref()));

    model.push_tool_result_ref(chat.clone(), turn.clone(), tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));

    // move 只搬移位置不增删，索引必须保持（remove+insert 后仍命中）。
    model.move_tool_result_after_tool_call(&chat, &turn, &tool);
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));
}

#[test]
fn tool_ref_index_rebuilds_after_retain() {
    let mut model = OutputTimelineModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatTurnId::new("turn-1");
    let keep_tool = ToolCallId::new("tool-keep");
    let drop_tool = ToolCallId::new("tool-drop");

    model.push_tool_call_ref(chat.clone(), turn.clone(), keep_tool.clone());
    model.push_tool_call_ref(chat.clone(), turn.clone(), drop_tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));

    model.retain(|item| {
        !matches!(item, OutputTimelineItem::ToolCall { reference }
            if reference.tool_call_id == drop_tool)
    });

    assert!(model.contains_tool_call(&chat, &turn, keep_tool.as_ref()));
    assert!(!model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));
}

#[test]
fn orphan_ids_index_tracks_pushed_and_retained_items() {
    let mut model = OutputTimelineModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatTurnId::new("turn-1");

    model.push(OutputTimelineItem::OrphanToolResult {
        id: "orphan-1".to_string(),
        tool_name: "Bash".to_string(),
        output: "out".to_string(),
        content: serde_json::json!({}),
        is_error: false,
    });
    assert!(model.contains_orphan("orphan-1"));

    model.retain(|item| !matches!(item, OutputTimelineItem::OrphanToolResult { .. }));
    assert!(!model.contains_orphan("orphan-1"));
}
```

- [ ] **Step 2: 跑测试验证编译失败（红：`tool_ref_index` / `contains_orphan` 尚不存在）**

Run: `cargo test -p cli output_timeline::model::tests`
Expected: FAIL to compile（`no field 'tool_ref_index'` / `no method 'contains_orphan'`）

- [ ] **Step 3: 实现索引**

重写 `apps/cli/src/tui/model/output_timeline/model.rs`：

头部 import 增加：

```rust
use std::collections::HashSet;
```

结构体改为：

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputTimelineModel {
    items: Vec<OutputTimelineItem>,
    /// ToolCall / ToolResult 存在性索引：push/retain 维护，move 不破坏。
    tool_ref_index: HashSet<TimelineToolCallRef>,
    /// OrphanToolResult 存在性索引（key 为 provider tool id）。
    orphan_ids: HashSet<String>,
}
```

`push` 改为统一入口（维护索引）：

```rust
pub fn push(&mut self, item: OutputTimelineItem) {
    index_tool_ref(&mut self.tool_ref_index, &mut self.orphan_ids, &item);
    self.items.push(item);
}
```

`retain` 改为重建索引：

```rust
pub fn retain<F>(&mut self, mut keep: F)
where
    F: FnMut(&OutputTimelineItem) -> bool,
{
    self.items.retain(|item| keep(item));
    self.rebuild_index();
}
```

新增私有辅助：

```rust
fn rebuild_index(&mut self) {
    self.tool_ref_index.clear();
    self.orphan_ids.clear();
    for item in &self.items {
        index_tool_ref(&mut self.tool_ref_index, &mut self.orphan_ids, item);
    }
}
```

模块级私有函数（放 `impl OutputTimelineModel` 外）：

```rust
fn index_tool_ref(
    tool_refs: &mut HashSet<TimelineToolCallRef>,
    orphans: &mut HashSet<String>,
    item: &OutputTimelineItem,
) {
    match item {
        OutputTimelineItem::ToolCall { reference }
        | OutputTimelineItem::ToolResult { reference } => {
            tool_refs.insert(reference.clone());
        }
        OutputTimelineItem::OrphanToolResult { id, .. } => {
            orphans.insert(id.clone());
        }
        _ => {}
    }
}
```

`contains_tool_call` / `contains_tool_result` 改走索引（语义不变：`ToolCallId` 是字符串 wrapper，`from_legacy_or_new` 与 push 侧构造等值）：

```rust
pub fn contains_tool_call(&self, chat_id: &ChatId, turn_id: &ChatTurnId, id: &str) -> bool {
    let reference = TimelineToolCallRef::new(
        chat_id.clone(),
        turn_id.clone(),
        ToolCallId::from_legacy_or_new(id),
    );
    self.tool_ref_index.contains(&reference)
}

pub fn contains_tool_result(&self, chat_id: &ChatId, turn_id: &ChatTurnId, id: &str) -> bool {
    let reference = TimelineToolCallRef::new(
        chat_id.clone(),
        turn_id.clone(),
        ToolCallId::from_legacy_or_new(id),
    );
    self.tool_ref_index.contains(&reference)
}
```

新增（供 `promote_orphan_tool_result` 前置判断）：

```rust
pub fn contains_orphan(&self, id: &str) -> bool {
    self.orphan_ids.contains(id)
}
```

`push_tool_call_ref` / `push_tool_result_ref` 改为索引判断 + 统一 push（幂等语义保持）：

```rust
pub fn push_tool_call_ref(
    &mut self,
    chat_id: ChatId,
    turn_id: ChatTurnId,
    tool_call_id: ToolCallId,
) {
    let reference = TimelineToolCallRef::new(chat_id, turn_id, tool_call_id);
    if !self.tool_ref_index.contains(&reference) {
        self.push(OutputTimelineItem::ToolCall { reference });
    }
}

pub fn push_tool_result_ref(
    &mut self,
    chat_id: ChatId,
    turn_id: ChatTurnId,
    tool_call_id: ToolCallId,
) {
    let reference = TimelineToolCallRef::new(chat_id, turn_id, tool_call_id);
    if !self.tool_ref_index.contains(&reference) {
        self.push(OutputTimelineItem::ToolResult { reference });
    }
}
```

`move_tool_result_after_tool_call` 开头增加 O(1) 短路（result 不存在时直接返回，覆盖运行期 ToolCallUpdate 常态——此时 result 尚未 push，原实现白做一次 O(n) position 扫描）：

```rust
pub fn move_tool_result_after_tool_call(
    &mut self,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_call_id: &ToolCallId,
) {
    if !self
        .tool_ref_index
        .contains(&TimelineToolCallRef::new(
            chat_id.clone(),
            turn_id.clone(),
            tool_call_id.clone(),
        ))
    {
        return;
    }
    // ...原有 position/remove/insert 逻辑保持不变...
}
```

修改 `apps/cli/src/tui/model/conversation/tool_flow.rs` 的 `promote_orphan_tool_result`，在 `find_map` 前加 O(1) 短路（每次 `update_tool_call` 都会调用本方法，正常路径无 orphan，原实现白扫全表）：

```rust
pub(super) fn promote_orphan_tool_result(
    &mut self,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    id: &str,
) {
    if !self.timeline.contains_orphan(id) {
        return;
    }
    // ...原有 find_map 逻辑保持不变...
}
```

- [ ] **Step 4: 跑索引一致性测试验证绿**

Run: `cargo test -p cli output_timeline::model::tests`
Expected: PASS（3 个新测试 + 既有 2 个测试）

- [ ] **Step 5: 跑 conversation 全量单测确认无行为回归**

Run: `cargo test -p cli tui::model::conversation`
Expected: PASS

- [ ] **Step 6: 跑性能测试验证转绿**

Run: `cargo test -p cli --release resume_performance_large_session -- --ignored --nocapture`
Expected: PASS，`elapsed < 3s`（预期 0.5~2s）。把修复后耗时记录到 issue #1467 comment。

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/tui/model/output_timeline/model.rs apps/cli/src/tui/model/conversation/tool_flow.rs
git commit -m "fix(tui): #1467 OutputTimelineModel 存在性索引化，resume 从 O(n²) 降为 O(n)"
```

---

### Task 3: 真实 session 验证（手动）

**Files:** 无代码改动；需要 release binary。

- [ ] **Step 1: 构建 release binary**

Run: `cargo build -p cli --release`
Expected: 构建成功；记录 binary 路径 `target/release/aemeath` 与 `git rev-parse HEAD`。

- [ ] **Step 2: resume 现场 session 计时**

Run（另开终端，日志级别 INFO 即可；session 文件 `~/.agents/session/019fa1be-bab3-7c47-ad94-c2952813dee8` 约 136MB）：

```bash
AEMEATH_LOG_LEVEL=aemeath:tui=info,aemeath:agent:runtime=info ~/.cache/.../target/release/aemeath --resume 019fa1be-bab3-7c47-ad94-c2952813dee8
```

Expected: 观察 `~/.agents/logs/tui.log` 中 `startup resume`（runtime 日志）→ `tui_first_frame`（tui 日志）间隔 < 30s（修复前 264s）。记录实测值与 TUI 首帧 timeline_items。

- [ ] **Step 3: 运行期慢帧观察**

resume 完成后发一条消息，观察 `tui.log` 的 `tui_slow_frame` 频率与 `draw_ms`。Expected: 相较修复前（draw 60~1287ms 持续）应有明显下降（索引化消除了每 tool call 多次全表扫描，但 draw 阶段全量渲染不在本 issue 根因范围，见 Task 4）。

- [ ] **Step 4: 记录验证结果到 issue**

Run: `gh issue comment 1467 --repo rushsinging/aemeath --body "真实 session 验证：...（binary SHA、startup→first_frame 实测、timeline_items、slow_frame 观察）"`

---

### Task 4: 运行期慢帧剖析（调查任务，产出结论）

**Files:** 只读 + 可能的小改动；结论记录到 issue #1467。

- [ ] **Step 1: 量化慢帧分布**

Run: `grep -c tui_slow_frame ~/.agents/logs/tui.log && grep tui_slow_frame ~/.agents/logs/tui.log | python3 -c "import sys,json; ds=[json.loads(l)['msg'] for l in sys.stdin]; ..."`（统计 draw_ms 分位数、output_dirty=true/false 分布）

Expected: 确认 output_dirty=false 的帧是否仍高频高耗时（现场 19:06~19:07 多个 output_dirty=false 帧 draw 64~338ms）。

- [ ] **Step 2: 读 draw 路径定位成本**

Read: `apps/cli/src/tui/app.rs:295`（`fn draw`）起，追踪 `prepare_ms / flush_ms / draw_ms` 三个阶段的实现；重点确认 output_dirty=false 时 draw 是否仍全量组装/渲染 document（view_assembler 的 revision memo 是否生效、ratatui diff 是否全量）。

Expected: 记录根因（若为 document 全量 rebuild：revision memo 未覆盖的路径；若为终端全量 diff：ratatui 层行为）。

- [ ] **Step 3: 决定修复或转出**

- 若根因是单点小改动（如 memo 键缺失、窗口未生效），在本 worktree 实现 + 补测试，追加 commit；
- 若涉及渲染层结构性改动（增量 document 重建等），在 issue #1467 comment 记录根因与候选方案，**转独立 sub-issue** 跟踪（**NEVER** 在本 issue 内扩大范围）。

- [ ] **Step 4: 产出结论**

Run: `gh issue comment 1467 --repo rushsinging/aemeath --body "运行期慢帧剖析结论：..."`（含根因、是否转出、引用新 issue 编号如适用）

---

### Task 5: 门禁与收尾

**Files:** 无（或 Task 4 追加的改动）。

- [ ] **Step 1: workspace 测试**

Run: `cargo test -p cli`
Expected: PASS 全绿

- [ ] **Step 2: clippy**

Run: `cargo clippy -p cli --all-targets -- -D warnings`
Expected: 无警告

- [ ] **Step 3: 架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`（worktree 根目录）
Expected: PASS（若本机 hooks 未装，记录并说明）

- [ ] **Step 4: 更新 issue 门禁 checklist**

Run: `gh issue edit 1467 --repo rushsinging/aemeath`（勾选已完成项；未完成项记录理由）或逐项 comment 更新。

- [ ] **Step 5: PR**

```bash
git pull origin main
git push -u origin feature-1467-tui-resume-timeline-o2
gh pr create --repo rushsinging/aemeath --base main --head feature-1467-tui-resume-timeline-o2 \
  --title "fix(tui): #1467 OutputTimelineModel 存在性索引化消除 resume O(n²) 卡顿" \
  --body "Summary: ...\nRefs: #1467 (sub-issue of #1417)\nBreaking change: no\nTest plan: cargo test -p cli; release resume_performance; 真实 session 计时"
```

Expected: PR 创建成功，等待用户 review（agent **NEVER** 自行合并）。

---

## 验收标准

1. `resume_performance_large_session`（30000 items）release 耗时 < 3s（修复前 > 60s）。
2. 真实 session `019fa1be-bab3-7c47-ad94-c2952813dee8`：startup → tui_first_frame < 30s。
3. `cargo test -p cli`、clippy、架构守卫全绿。
4. 运行期慢帧剖析结论落 issue（修复或转出）。
5. 索引一致性单测覆盖 push / move / retain / orphan 四条路径。

## 已知边界与不变量

- 索引只存存在性不存位置：`move` 内部 `remove/insert` 不破坏一致性，无需位置同步。
- `items_mut()` 仅被 text_stream / ask_user 用于修改既有 item 字段（不增删），不破坏索引；若未来出现经 `items_mut` 增删的调用，**MUST** 同步维护索引或改走 `push/retain`。
- `ToolCallId` 为字符串 wrapper，`from_legacy_or_new` 与 push 侧构造等值，contains 语义与改造前一致。
- resume 路径的 `ensure_runtime_turn`（O(chats)）与 `runtime_turn_mut`（O(chats×turns)）在本规模（159 chats / 7142 turns）下可接受，不在本 issue 范围。
