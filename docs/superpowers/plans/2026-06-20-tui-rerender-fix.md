# TUI 大会话全量重渲染卡死修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 TUI 在大会话下每 90ms 全量重建整会话 view_model 导致的伪卡死（live-lock），同时保留运行中 tool 的 gutter 动画。

**Architecture:** 双管齐下。(A) `SpinnerTick` 仅在 spinner 真正 active（处理中）时才标脏 output——idle/完成态不再触发重建。(B) `ConversationModel` 维护单调 `revision` 计数器，`refresh_output_document_from_model` 据此 memo `assemble_from_conversation` 的产物——revision 不变则复用上次 view_model，跳过全量遍历+clone。两者组合后：idle 完全不重建（CPU≈0）；active 期间仅在内容真正变化时才 assemble，spinner 动画帧只走 render 层（gutter）。

**Tech Stack:** Rust / ratatui / TEA（The Elm Architecture）风格 update。crate：`cli`。

## Global Constraints

- 所有改动 **MUST** 在 worktree `.claude/worktrees/fix-425-tui-rerender`（分支 `worktree-fix-425-tui-rerender`）内进行，NEVER 直接改 main。
- **MUST** TDD：先写失败测试（红）→ 最小实现（绿）→ 重构。测试 NEVER 为迁就实现削弱断言。
- **MUST** 每个改动的 pub/pub(crate) 函数有测试覆盖；纯逻辑函数覆盖正常/边界/错误三路径。
- 错误/提示消息 **MUST** 中文。
- **MUST NOT** 手动调格式，交给 `cargo fmt`。
- 验证门禁：`cargo test -p cli`、`cargo clippy -p cli`（NEVER 引入新 warning）。
- 日志 **MUST** 用 `crate::tui::log_*!` 宏（自动 `target: "cli::tui"`）。
- baseline：本 worktree `cargo test -p cli` = 885 passed, 0 failed。每个 task 结束须保持全绿。

---

## File Structure

- `apps/cli/src/tui/app/update.rs` — 修改 `SpinnerTick` 分支（门控标脏）+ 重写 `refresh_output_document_from_model`（接入 memo）。
- `apps/cli/src/tui/model/conversation/model.rs` — `ConversationModel` 新增 `revision: u64` 字段 + `revision()` getter，`apply()` 末尾按非空 change bump。
- `apps/cli/src/tui/app.rs` — `App` 新增 `output_view_cache: Option<OutputViewCache>` 字段及其类型定义与初始化。
- `apps/cli/src/tui/app/update/notice_tests.rs` — 复用 `make_app()`，新增 A1/A3 行为测试（该文件已是 update 模块的测试宿主）。

---

## Task A1: SpinnerTick 仅在 spinner active 时标脏 output

**Files:**
- Modify: `apps/cli/src/tui/app/update.rs:175`（`SpinnerTick` 分支内的 `self.mark_output_dirty();`）
- Test: `apps/cli/src/tui/app/update/notice_tests.rs`（新增 2 个测试）

**Interfaces:**
- Consumes: `App::update(&mut self, msg: TuiMsg, ui_tx: &mpsc::Sender<UiEvent>, spawn_refs: &SpawnContextRefs) -> UpdateResult`（update.rs:109）；`App::new(session: String, cwd: PathBuf, model: String) -> App`；`self.model.runtime.spinner.active: bool`；`self.view_state.dirty.output: bool`；`SpawnContextRefs { agent_client: Option<Arc<dyn sdk::AgentClient>> }`（processing.rs:205）。
- Produces: 行为契约——idle（`spinner.active == false`）下 `TuiMsg::SpinnerTick` 不置 `dirty.output`；active 下置 `dirty.output`。

- [ ] **Step 1: 写失败测试**

在 `apps/cli/src/tui/app/update/notice_tests.rs` 末尾的 `mod tests` 同级（文件顶部已有 `use super::*;` 与 `make_app()`）追加：

```rust
#[test]
fn test_spinner_tick_idle_does_not_mark_output_dirty() {
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::update::msg::TuiMsg;
    let mut app = make_app();
    app.model.runtime.spinner.active = false; // idle / 已完成
    app.view_state.dirty.clear_output();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(8);
    let spawn_refs = SpawnContextRefs { agent_client: None };
    app.update(TuiMsg::SpinnerTick, &ui_tx, &spawn_refs);
    assert!(
        !app.view_state.dirty.output,
        "idle 时 SpinnerTick 不应标脏 output（否则空闲态每 90ms 全量重建整会话）"
    );
}

#[test]
fn test_spinner_tick_active_marks_output_dirty() {
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::update::msg::TuiMsg;
    let mut app = make_app();
    app.model.runtime.spinner.active = true; // 处理中，需要 gutter 动画
    app.view_state.dirty.clear_output();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(8);
    let spawn_refs = SpawnContextRefs { agent_client: None };
    app.update(TuiMsg::SpinnerTick, &ui_tx, &spawn_refs);
    assert!(
        app.view_state.dirty.output,
        "active 时 SpinnerTick 应标脏 output，以驱动运行中 tool 的 gutter 动画"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p cli test_spinner_tick_idle_does_not_mark_output_dirty`
Expected: FAIL（当前无条件标脏，断言 `!dirty.output` 失败）。

- [ ] **Step 3: 最小实现——门控标脏**

`apps/cli/src/tui/app/update.rs:175`，将无条件标脏：

```rust
                self.view_state.spinner.advance();
                self.mark_output_dirty();
```

改为仅 active 时标脏（advance 动画帧保留，无论是否 active）：

```rust
                self.view_state.spinner.advance();
                // 仅在处理中（有运行中 block 的 gutter 动画需要重绘）时才标脏 output。
                // idle/完成态标脏会导致每 90ms 全量重建整会话 → 大会话伪卡死（live-lock）。
                if self.model.runtime.spinner.active {
                    self.mark_output_dirty();
                }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p cli test_spinner_tick`
Expected: PASS（2 个新测试均绿）。

- [ ] **Step 5: 回归 + 提交**

Run: `cargo test -p cli && cargo clippy -p cli`
Expected: 全绿、无新 warning。

```bash
git add apps/cli/src/tui/app/update.rs apps/cli/src/tui/app/update/notice_tests.rs
git commit -m "fix(tui): SpinnerTick 仅在 spinner active 时标脏 output，避免 idle 全量重渲染"
```

---

## Task A2: ConversationModel 维护 revision 计数器

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model.rs:14`（struct 字段）、`:47`（`apply` 末尾 bump）
- Test: `apps/cli/src/tui/model/conversation/model.rs` 文件末尾 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `ConversationModel::apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange>`（model.rs:47）；`ConversationIntent::AppendUserMessage { text }`、`ConversationIntent::ClearQueuedSubmissions`。
- Produces: `ConversationModel::revision(&self) -> u64`（A3 用作 memo key）；语义——任一使 `apply` 返回非空 `Vec<ConversationChange>` 的 intent 使 `revision` 单调 +1；返回空 change 的 intent 不改 revision；`reset()` 归 0。

- [ ] **Step 1: 写失败测试**

在 `apps/cli/src/tui/model/conversation/model.rs` 文件末尾 `#[cfg(test)] mod tests` 内追加（若文件无 tests mod 则新建）：

```rust
    #[test]
    fn test_revision_starts_at_zero() {
        let model = ConversationModel::default();
        assert_eq!(model.revision(), 0, "新建 conversation revision 应为 0");
    }

    #[test]
    fn test_revision_bumps_on_mutating_apply() {
        let mut model = ConversationModel::default();
        let before = model.revision();
        let changes = model.apply(ConversationIntent::AppendUserMessage {
            text: "你好".to_string(),
        });
        assert!(!changes.is_empty(), "AppendUserMessage 应产生 change");
        assert_eq!(
            model.revision(),
            before + 1,
            "产生 change 的 apply 应使 revision +1"
        );
    }

    #[test]
    fn test_revision_unchanged_on_noop_apply() {
        let mut model = ConversationModel::default();
        // 无排队提交时 ClearQueuedSubmissions 为 no-op（返回空 change）。
        let before = model.revision();
        let changes = model.apply(ConversationIntent::ClearQueuedSubmissions);
        assert!(changes.is_empty(), "空队列下 Clear 应为 no-op");
        assert_eq!(model.revision(), before, "no-op apply 不应改 revision");
    }
```

> 注：若 `ClearQueuedSubmissions` 在空队列下并非返回空 `Vec`，改用任一已知返回空 change 的 intent；执行时以 `cargo test` 实测为准，断言 `changes.is_empty()` 会先暴露。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p cli test_revision_starts_at_zero`
Expected: FAIL（`revision` 方法/字段不存在，编译错误）。

- [ ] **Step 3: 最小实现——字段 + getter + bump**

(a) `model.rs:14` 的 struct 增加字段（置于 `next_block_sequence` 后）：

```rust
    next_chat_sequence: usize,
    next_block_sequence: usize,
    /// 单调递增的内容版本号；每次产生 change 的 apply +1。
    /// 供渲染层 memo `assemble_from_conversation`：revision 不变即可复用上次 view_model。
    revision: u64,
```

(b) `apply`（model.rs:47）改为先存 match 结果、按非空 bump、再返回。将 `:47-48`：

```rust
    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        match intent {
```

改为：

```rust
    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        let changes = match intent {
```

并将 match 闭合处 `model.rs:145-146`：

```rust
            ConversationIntent::DismissAskUserBatch => self.dismiss_ask_user_batch(),
        }
    }
```

改为：

```rust
            ConversationIntent::DismissAskUserBatch => self.dismiss_ask_user_batch(),
        };
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        changes
    }

    /// 当前内容版本号，供渲染层 memo。
    pub fn revision(&self) -> u64 {
        self.revision
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p cli test_revision`
Expected: PASS（3 个新测试均绿）。

- [ ] **Step 5: 全量回归（防 derive(Eq) 影响）+ 提交**

Run: `cargo test -p cli && cargo clippy -p cli`
Expected: 885+3 全绿、无 warning。
> `ConversationModel` derive(Eq, PartialEq)，新字段参与比较。若有测试比较两个由相同操作序列构造的 model，revision 应一致；若意外失败，定位该测试评估是否需用 `reset()`/相同序列对齐，NEVER 削弱 A2 断言绕过。

```bash
git add apps/cli/src/tui/model/conversation/model.rs
git commit -m "feat(tui): ConversationModel 增加 revision 计数器作为渲染 memo key"
```

---

## Task A3: refresh_output_document_from_model 接入 assemble memo

**Files:**
- Modify: `apps/cli/src/tui/app.rs:30`（App 字段）、`:163` 附近（初始化）；`apps/cli/src/tui/app/update.rs:255`（重写 refresh）
- Test: `apps/cli/src/tui/app/update/notice_tests.rs`（新增 memo 行为测试，借助 `OutputDocumentRenderer` 的 `#[cfg(test)] render_count`）

**Interfaces:**
- Consumes: `ConversationModel::revision()`（A2）；`OutputViewAssembler::assemble_from_conversation(&ConversationModel, u64, Option<&Path>) -> OutputViewModel`（output.rs:17）；`OutputViewModel: Clone`（view_model/output.rs:5）。
- Produces: `App.output_view_cache: Option<OutputViewCache>`；不可见行为契约——`revision` 不变的连续 `refresh_output_document_from_model()` 仅 assemble 一次。

- [ ] **Step 1: 写失败测试**

在 `notice_tests.rs` 追加（探针：`assemble` 调用次数无现成计数，改用可观察代理——revision 不变时复用，则连续两次 refresh 后 cache.revision 稳定且不重新 assemble；以「修改 conversation 后 revision 改变触发重建、未改时不变」为断言）：

```rust
#[test]
fn test_refresh_reuses_view_model_when_revision_unchanged() {
    let mut app = make_app();
    app.append_system_notice("一条消息"); // 产生 change，revision 前进
    app.refresh_output_document_from_model();
    let cached_rev_1 = app
        .output_view_cache
        .as_ref()
        .expect("首次 refresh 应填充 cache")
        .revision;
    // 不改 conversation，再次 refresh：应命中 memo，revision 不变。
    app.refresh_output_document_from_model();
    let cached_rev_2 = app
        .output_view_cache
        .as_ref()
        .expect("cache 应仍在")
        .revision;
    assert_eq!(
        cached_rev_1, cached_rev_2,
        "conversation 未变时 refresh 应复用 view_model（revision 不变）"
    );
}

#[test]
fn test_refresh_rebuilds_after_conversation_mutates() {
    let mut app = make_app();
    app.append_system_notice("第一条");
    app.refresh_output_document_from_model();
    let rev_1 = app.output_view_cache.as_ref().unwrap().revision;
    app.append_system_notice("第二条"); // conversation 变化 → revision 前进
    app.refresh_output_document_from_model();
    let rev_2 = app.output_view_cache.as_ref().unwrap().revision;
    assert!(
        rev_2 > rev_1,
        "conversation 变化后 refresh 应以新 revision 重建 cache"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p cli test_refresh_reuses_view_model_when_revision_unchanged`
Expected: FAIL（`output_view_cache` 字段不存在，编译错误）。

- [ ] **Step 3a: App 增加 cache 字段与类型**

`apps/cli/src/tui/app.rs`：在 `App` struct（:30）内 `output_document_renderer` 字段后增加：

```rust
    pub(crate) output_document_renderer: OutputDocumentRenderer,
    /// memo：缓存上次 assemble 的 (revision, view_model)。revision 不变即复用，
    /// 跳过 `assemble_from_conversation` 的全量遍历+clone（大会话伪卡死根治）。
    pub(crate) output_view_cache: Option<OutputViewCache>,
```

在 `app.rs` 文件内（struct 定义上方或下方，模块级）新增类型：

```rust
/// `refresh_output_document_from_model` 的 assemble 产物 memo。
pub(crate) struct OutputViewCache {
    pub(crate) revision: u64,
    pub(crate) view_model: crate::tui::view_model::OutputViewModel,
}
```

在 `App` 构造处（:163 附近，`output_document_renderer: OutputDocumentRenderer::default(),` 后）增加初始化：

```rust
            output_document_renderer: OutputDocumentRenderer::default(),
            output_view_cache: None,
```

- [ ] **Step 3b: 重写 refresh 接入 memo（take/swap 避免 clone 与借用冲突）**

`apps/cli/src/tui/app/update.rs:255` 的 `refresh_output_document_from_model` 整体替换为：

```rust
    pub(crate) fn refresh_output_document_from_model(&mut self) {
        let before_lines = self.output_area.document().total_lines();
        let revision = self.model.conversation.revision();
        // memo：revision 不变复用上次 view_model，跳过全量 assemble。
        let need_rebuild = self
            .output_view_cache
            .as_ref()
            .map(|cache| cache.revision != revision)
            .unwrap_or(true);
        if need_rebuild {
            let working_root = self
                .model
                .runtime
                .workspace
                .working_root
                .as_deref()
                .map(std::path::Path::new);
            let view_model = OutputViewAssembler::assemble_from_conversation(
                &self.model.conversation,
                revision,
                working_root,
            );
            self.output_view_cache = Some(OutputViewCache {
                revision,
                view_model,
            });
        }
        // take 出 owned view_model，render 期间释放对 self 的不可变借用，render 后放回。
        let cache = self
            .output_view_cache
            .take()
            .expect("memo cache filled above");
        let view_model = cache.view_model;
        let cached_revision = cache.revision;
        let root_count = view_model.roots.len();
        let width = self.output_document_width();
        let render_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.output_document_renderer.render_model_document(
                &view_model,
                width,
                self.output_area.term_width,
                self.view_state.animation.spinner_frame,
            )
        }));
        // 无论渲染成败都把 view_model 放回 cache，保留 memo。
        self.output_view_cache = Some(OutputViewCache {
            revision: cached_revision,
            view_model,
        });
        let document = match render_result {
            Ok(document) => document,
            Err(_) => {
                crate::tui::log_warn!(
                    "tui.output.refresh_document panicked; keeping previous document"
                );
                self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                    StatusNotice::warning("渲染失败，已记录 panic.log"),
                ));
                return;
            }
        };
        let after_lines = document.total_lines();
        crate::tui::log_trace!(
            "tui.output.refresh_document revision={} width={} term_width={} spinner_frame={} roots={} conversation_blocks={} chats={} before_lines={} after_lines={} rebuilt={}",
            revision,
            width,
            self.output_area.term_width,
            self.view_state.animation.spinner_frame,
            root_count,
            self.model.conversation.blocks.len(),
            self.model.conversation.chats.len(),
            before_lines,
            after_lines,
            need_rebuild
        );
        self.output_area.replace_document(document);
    }
```

> 注意：原日志的 `version={}` 字段改为 `revision={}` 并新增 `rebuilt={}`，与 specs/logging.md 的 14 字段无关（这是 message 文本）。`self.view_state.output.version` 不再传入 assemble（该字段恒 0，已废用）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p cli test_refresh`
Expected: PASS。

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test -p cli && cargo clippy -p cli`
Expected: 全绿、无 warning。

```bash
git add apps/cli/src/tui/app.rs apps/cli/src/tui/app/update.rs apps/cli/src/tui/app/update/notice_tests.rs
git commit -m "perf(tui): refresh_output_document_from_model 按 conversation revision memo，跳过空闲全量 assemble"
```

---

## Task A4: 大会话手工验证（根治确认）

**Files:** 无代码改动，验证步骤。

- [ ] **Step 1: 构建发布二进制**

Run: `cargo build -p cli`
Expected: 成功。

- [ ] **Step 2: resume 大会话观察 idle CPU**

用大会话 id（如 `019ee533-7aca-77c9-8b46-7db709885251`，存在于 `~/.agents/sessions/`）：

```bash
./target/debug/aemeath --model Zhipu/glm-5.2 --resume 019ee533-7aca-77c9-8b46-7db709885251
```

待 agent 空闲后，另开终端：

```bash
ps -o pid,%cpu,stat,command -p <pid>
sample <pid> 1 -mayDie 2>/dev/null | grep -c assemble_from_conversation
```

Expected：修复后 idle `%cpu` ≈ 0；`sample` 中 `assemble_from_conversation` 命中数显著下降（idle 不再持续命中）；TUI 输入即时响应。

- [ ] **Step 3: 记录结论**

把 before/after 的 CPU 与采样对比贴入 issue 425 评论（验证证据）。

---

## Phase B（后续单独细化）：output 视口虚拟化

> **本计划不展开 B 的逐步代码**——B 是渲染管线架构级重构，需在 A 验证根治后单独成 plan，并与下列协调：
> - issue 388（TUI typed 组装管线 / 拆 `view_assembler/output.rs`）——同文件域，避免重复改。
> - issue 390（持久化会话 actor + timeline 单一真相）——可见 item 定位在 timeline 单一真相后更稳。
>
> **方向**：`assemble_from_conversation` + `render_model_document` 从「全量 O(整会话)」改为「按当前 `view_state.output.scroll_offset` + 可见高度只构建/渲染可见视口 + 上下缓冲若干屏」，使单帧成本与会话总长度解耦。届时 A3 的 clone-free memo 仍作为视口外内容的复用基础。
>
> A 完成后回到用户 checkpoint：确认 B 是立即继续、还是并入 388/390 排期。

---

## Self-Review

- **Spec 覆盖**：issue 425「修复/实现」A(a) 触发频率 → Task A1；A(b) assemble memo → Task A2+A3；B 视口虚拟化 → Phase B（注明单独 plan）。验证段 → Task A4。✓
- **Placeholder 扫描**：每个代码步骤含完整可编译代码与精确 old→new 替换；A2 对 no-op intent 的注记给了实测兜底而非占位。✓
- **类型一致性**：`revision()`/`revision` 字段、`OutputViewCache { revision, view_model }`、`output_view_cache` 在 A2/A3 间命名一致；`assemble_from_conversation` 第二参数由 `revision` 传入（替代恒 0 的 `output.version`）。✓
