# AskUserQuestion TUI Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 AskUserQuestion 自由输入焦点、已回答摘要缩进和 resume 回填不一致，并用模型、组装、渲染及 TUI 场景测试锁定完整链路。

**Architecture:** `ConversationModel` 是 Ask 展示状态的唯一来源；每个 Ask 批次使用首个 tool call/slot id 派生稳定 id，历史确认批次可并存，但最多保留一个未确认活动批次。实时事件统一走 `AskUserState`，resume 从 assistant tool use 与紧随其后的 user tool result 配对恢复确认批次，`ViewAssembler` 和 renderer 只消费模型状态。

**Tech Stack:** Rust、Tokio oneshot、Ratatui、现有 `TuiScenarioHarness`、Insta snapshot、Cargo workspace。

---

## File structure

- Modify: `apps/cli/src/tui/model/conversation/ask_user.rs`
  - 生成稳定批次 id、只替换活动批次、恢复确认批次、活动快照与无选项自由输入状态。
- Create: `apps/cli/src/tui/model/conversation/ask_user_tests.rs`
  - 迁移相关内联测试并新增稳定 id、多确认批次、活动批次选择与导航测试。
- Modify: `apps/cli/src/tui/model/conversation/history_parse.rs`
  - 保留 tool result `text` 并集中实现 Ask 答案提取的兼容顺序。
- Create: `apps/cli/src/tui/model/conversation/history_parse_tests.rs`
  - 迁移相关内联测试并覆盖 `content.answer`、`text`、字符串 content、错误与畸形输入。
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
  - `ResumeConversation` 按 assistant message 恢复全部成功 Ask 批次。
- Create: `apps/cli/src/tui/model/conversation/intent_impls_tests.rs`
  - 迁移相关内联测试并覆盖 resume 的模型投影。
- Modify: `apps/cli/src/tui/app/state/input.rs`
  - 删除重复的单问题 reply sender 状态。
- Modify: `apps/cli/src/tui/app/update/ui_event.rs`
  - 所有 AskUserBatch 统一构造 `AskUserState`。
- Modify: `apps/cli/src/tui/app/update/ask_user_key.rs`
  - 删除全局 InputDocument 的 Ask 特例，所有输入编辑与提交均操作 Ask block。
- Modify: `apps/cli/src/tui/app/scenario_tests/interaction.rs`
  - 增加自由输入归属、确认帧与 resume 多批次的 L4 场景。
- Modify: `apps/cli/src/tui/view_assembler/output/tests.rs`
  - 覆盖多个已确认 Ask 批次按序投影。
- Modify: `apps/cli/src/tui/render/output/blocks/ask_user.rs`
  - 使用已确认的 A 布局缩进。
- Create: `apps/cli/src/tui/render/output/blocks/ask_user_tests.rs`
  - 迁移相关内联测试并精确断言摘要缩进及确认页 marker。
- Modify: `apps/cli/src/tui/app/scenario_tests/snapshots/*.snap`
  - 仅在语义断言通过后更新受影响的画面快照。

### Task 1: ConversationModel 支持稳定 id、多历史批次和无选项输入

- [ ] 在 `ask_user_tests.rs` 迁移 `ask_user.rs` 内联测试，并在源文件用 `#[path = "ask_user_tests.rs"] mod tests;` 引入，确保仅迁移时测试仍通过。
- [ ] 新增失败测试：
  - `show_no_option_batch_activates_chat_input`
  - `show_batch_uses_first_slot_id_for_stable_identity`
  - `show_new_batch_preserves_confirmed_batches_and_replaces_only_active_batch`
  - `ask_user_snapshot_targets_active_unconfirmed_batch`
  - `answering_next_no_option_slot_activates_chat_input`
  - `navigate_back_to_no_option_slot_activates_chat_input`
  - `restore_answered_batch_appends_confirmed_history`
- [ ] 运行：

```bash
cargo test -p cli tui::model::conversation::ask_user::tests -- --nocapture
```

预期：新测试因固定 `ask-user` id、删除所有旧批次和 `chat_input_active = false` 失败。

- [ ] 在 `ask_user.rs` 实现小型共享 helper：

```rust
fn ask_user_block_id(slots: &[AskUserSlot]) -> String {
    slots
        .first()
        .map(|slot| format!("ask-user-{}", slot.id))
        .unwrap_or_else(|| "ask-user-empty".to_string())
}

fn slot_starts_in_chat_input(slot: Option<&AskUserSlot>) -> bool {
    slot.is_some_and(|slot| slot.options.is_empty())
}
```

- [ ] 将查找可编辑块的 helper 改为只返回 `confirmed == false` 的 Ask block；展示新批次时仅移除未确认块。
- [ ] `show_ask_user_batch`、前进下一题、确认页回退统一调用 `slot_starts_in_chat_input`。
- [ ] 新增 `restore_answered_ask_user_batch(slots)`，追加 `confirmed = true` 批次且不影响其它批次。
- [ ] 重新运行 Task 1 测试，预期全绿。
- [ ] 提交：

```bash
git add apps/cli/src/tui/model/conversation/ask_user.rs apps/cli/src/tui/model/conversation/ask_user_tests.rs
git commit -m "fix(tui): #1358 统一 Ask 批次状态"
```

### Task 2: 实时自由输入统一写入 Ask block

- [ ] 在 `interaction.rs` 新增失败场景 `ask_user_free_text_stays_in_ask_block`：
  - 发出单个无 options 的 `UiEvent::AskUserBatch`
  - 键入 `222`
  - 语义断言 Ask block 的 `chat_input_text == "222"`
  - 语义断言底部 `InputDocument` 为空
  - Enter 后断言 oneshot 收到 `Answers(["222"])`
  - 渲染并断言确认摘要包含答案
- [ ] 运行：

```bash
cargo test -p cli tui::app::scenario_tests::interaction::ask_user_free_text_stays_in_ask_block -- --exact --nocapture
```

预期：输入进入全局 InputDocument，Ask block 文本仍为空，测试失败。

- [ ] 修改 `ui_event.rs`，删除单个无 options 分支，所有批次都：

```rust
self.input.ask_user_state = Some(AskUserState { reply_tx, items });
self.show_ask_user_batch(slots);
```

- [ ] 从 `InputState` 删除 `ask_user_reply_tx`。
- [ ] 从 `ask_user_key.rs` 删除 `ask_user_reply_tx` 分支和 `update_ask_user_input_key`，确保按键只依据活动 Ask snapshot 路由。
- [ ] 更新受字段删除影响的外部测试/初始化代码，不复制 sender 状态。
- [ ] 重跑场景和相关 update 单元测试：

```bash
cargo test -p cli tui::app::scenario_tests::interaction::ask_user_free_text_stays_in_ask_block -- --exact --nocapture
cargo test -p cli tui::app::update::ask_user_key::tests -- --nocapture
cargo test -p cli tui::app::update::ui_event::tests -- --nocapture
```

预期：全部通过。

- [ ] 提交：

```bash
git add apps/cli/src/tui/app/state/input.rs apps/cli/src/tui/app/update/ui_event.rs apps/cli/src/tui/app/update/ask_user_key.rs apps/cli/src/tui/app/scenario_tests/interaction.rs
git commit -m "fix(tui): #1358 聚合 Ask 自由输入"
```

### Task 3: 历史解析兼容 Ask 答案格式

- [ ] 将 `history_parse.rs` 内联测试迁移至 `history_parse_tests.rs`，迁移后先跑原测试确认无行为变化。
- [ ] 新增失败测试，驱动单一解析 helper：

```rust
pub(crate) fn restored_ask_answer(result: HistoryToolResult<'_>) -> Option<String>
```

覆盖：
  - object content 的 `answer`
  - top-level ToolResult `text`
  - string content
  - `is_error == true`
  - 空答案
  - object 缺少 `answer`
  - 非字符串 `answer`

- [ ] 运行：

```bash
cargo test -p cli tui::model::conversation::history_parse::tests -- --nocapture
```

预期：helper 尚不存在或兼容行为尚未实现，编译/断言失败。

- [ ] 给 `HistoryToolResult` 增加 `text: Option<&str>`；`collect_following_tool_results` 从 `ContentBlock::ToolResult` 原样转发 `text`。
- [ ] 实现答案优先级：非空 `content.answer` → 非空 `text` → 非空字符串 content；错误结果返回 `None`。
- [ ] 重跑 Task 3 测试，预期全绿。
- [ ] 提交：

```bash
git add apps/cli/src/tui/model/conversation/history_parse.rs apps/cli/src/tui/model/conversation/history_parse_tests.rs
git commit -m "fix(tui): #1358 解析历史 Ask 答案"
```

### Task 4: ResumeConversation 恢复全部成功 Ask 批次

- [ ] 将 `intent_impls.rs` 内联测试迁移至 `intent_impls_tests.rs`。
- [ ] 新增失败测试：
  - 一个 assistant message 内多个成功 Ask tool use 恢复成一个 confirmed batch
  - 多个 assistant message 的 Ask 恢复成多个 confirmed batch且顺序不变
  - 缺结果、错误结果、问题字段畸形继续保留 generic tool call/result，不中断 resume
- [ ] 运行：

```bash
cargo test -p cli tui::model::conversation::intent_impls::tests -- --nocapture
```

预期：resume 只有 generic tool rows，没有 confirmed Ask batch，测试失败。

- [ ] 在 `ResumeConversation` 遍历每个 assistant message 时：
  - 继续使用 `collect_following_tool_results`
  - generic tool call/result 恢复逻辑不变
  - 对 `name == "AskUserQuestion"`、非空 `input.question`、成功匹配答案的项构造 `AskUserSlot`
  - 每个 assistant message 收集完成后调用 `restore_answered_ask_user_batch`
  - 解析失败仅跳过摘要恢复，不返回整个 resume 错误
- [ ] 重跑 Task 4 测试，预期全绿。
- [ ] 提交：

```bash
git add apps/cli/src/tui/model/conversation/intent_impls.rs apps/cli/src/tui/model/conversation/intent_impls_tests.rs
git commit -m "fix(tui): #1358 恢复历史 Ask 摘要"
```

### Task 5: ViewAssembler 和 renderer 锁定多批次与 A 布局

- [ ] 在 `view_assembler/output/tests.rs` 新增失败测试：两个 confirmed Ask model items 映射成两个 `AskUserBatchBlockView`，id、问题、答案和顺序保持不变。
- [ ] 将 renderer 内联测试迁移至 `ask_user_tests.rs`，新增精确文本失败测试：

```text
━━ 已回答 ━━

Q1. 你打算吃什么？
  ❯ 日料吧
```

并保留一个 confirming 测试，断言当前问题 marker 未被已回答布局修改影响。

- [ ] 运行：

```bash
cargo test -p cli tui::view_assembler::output::tests -- --nocapture
cargo test -p cli tui::render::output::blocks::ask_user::tests -- --nocapture
```

预期：多批次投影测试视已有 assembler 行为决定；摘要缩进测试因当前额外空格失败。

- [ ] 修改 `qa_summary_lines`：Q 从内容区起点开始，A 固定缩进一级；不改 confirming marker 和宽度计算语义。
- [ ] 若 assembler 过滤/覆写批次，仅做最小修复以逐项保留。
- [ ] 重跑 Task 5 测试，预期全绿。
- [ ] 提交：

```bash
git add apps/cli/src/tui/view_assembler/output/tests.rs apps/cli/src/tui/render/output/blocks/ask_user.rs apps/cli/src/tui/render/output/blocks/ask_user_tests.rs
git commit -m "fix(tui): #1358 收紧 Ask 摘要布局"
```

### Task 6: L4 场景覆盖确认与 resume

- [ ] 扩展 `interaction.rs`：
  - `ask_user_free_text_stays_in_ask_block` 增加确认后的 A 布局语义断言
  - `resume_restores_all_answered_ask_batches` 构造两个 assistant Ask + following user ToolResult 对，触发 `ResumeConversation`，断言模型与 framebuffer 同时出现两批问题/答案且顺序正确
- [ ] 先运行语义测试，确认实现通过：

```bash
cargo test -p cli tui::app::scenario_tests::interaction::ask_user_free_text_stays_in_ask_block -- --exact --nocapture
cargo test -p cli tui::app::scenario_tests::interaction::resume_restores_all_answered_ask_batches -- --exact --nocapture
```

- [ ] 语义断言通过后才更新相关 snapshot：

```bash
INSTA_UPDATE=always cargo test -p cli tui::app::scenario_tests::interaction -- --nocapture
```

- [ ] 检查 snapshot diff，确认只包含输入归属、A 布局及 resume 多摘要。
- [ ] 提交：

```bash
git add apps/cli/src/tui/app/scenario_tests/interaction.rs apps/cli/src/tui/app/scenario_tests/snapshots
git commit -m "test(tui): #1358 覆盖 Ask 输入与恢复场景"
```

### Task 7: 清理、全量门禁与 PR

- [ ] 搜索并确认无遗留重复路径：

```bash
rg -n "ASK_USER_BLOCK_ID|ask_user_reply_tx|update_ask_user_input_key" apps/cli/src
```

预期：旧固定 id、重复 sender 和全局输入 helper 均无引用。

- [ ] 运行格式化并检查 diff：

```bash
cargo fmt --all
git diff --check
```

- [ ] 运行目标包验证：

```bash
cargo build -p cli
cargo test -p cli
cargo clippy -p cli --all-targets --all-features -- -D warnings
```

- [ ] 运行仓库架构守卫：

```bash
bash .agents/hooks/check-architecture-guards.sh
```

- [ ] 记录测试分层：L0/L1/L2/L4 已覆盖；L3 不涉及真实 runtime/provider 边界；L5 不涉及真实终端输入、信号或外部基础设施，因此 N/A。
- [ ] 使用 `superpowers:requesting-code-review` 做 diff 复核，修复发现后重跑相关验证。
- [ ] 使用 `superpowers:verification-before-completion` 基于最新 HEAD 重跑必要命令并保存结果。
- [ ] 更新 issue #1358 checklist、验证结果与 PR 关联，但不关闭 issue。
- [ ] 提交任何格式化/复核修正：

```bash
git add -A
git commit -m "chore(tui): #1358 完成 Ask 修复验证"
```

仅在确有未提交变更时执行。

- [ ] 推送并创建 PR：

```bash
git push -u origin fix/1358-ask-user-tui-consistency
gh pr create --base main --head fix/1358-ask-user-tui-consistency \
  --title "fix(tui): unify Ask input and resume summaries" \
  --body-file /tmp/aemeath-pr-1358.md
```

- [ ] 检查 PR 页面/CI 状态并向用户报告 PR 链接、提交、验证证据及未执行的 L3/L5 理由；不合并 PR。
