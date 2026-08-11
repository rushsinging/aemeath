# Issue #742 Run Status Single Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Runtime typed Run 状态无损传到 TUI 状态镜像，以此纯派生活动展示和 Main 模型静默占位，并删除第二生命周期与 `ModelStreamWaiting`。

**Architecture:** Runtime `RunStatus` 通过 SDK `RunStatusView` 和 TUI ACL 进入 `RunStateSnapshot`；ViewState 的 `RunActivityState` 只保存 Main Run 单调展示时间与动画，ViewAssembler 联合两者产出 `RunActivityView`。内容、Tool、Hook、Compact 事件只更新内容或 detail，不再拥有活动生命周期。

**Tech Stack:** Rust 2021、Tokio、Ratatui、serde/schemars、现有 TUI TEA/ACL/ViewAssembler 测试 Harness。

---

## 文件结构

- `packages/sdk/src/chat_event.rs`：定义 `RunStatusView` 并用于 `ChatEvent::RunTransitioned`。
- `packages/sdk/src/chat_event_tests.rs`：锁定 typed Published Language 的全变体与 serde shape。
- `packages/sdk/src/lib.rs`、`packages/sdk/src/chat.rs`：re-export `RunStatusView`。
- `agent/features/runtime/src/adapters/sdk_event_mapper.rs`：穷举映射 Runtime `RunStatus`。
- `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs`：全变体 Runtime→SDK 契约。
- `agent/features/runtime/src/application/model/invocation.rs`、`application/loop_engine/chat/{events.rs,stream_handler.rs,stream_handler_tests.rs,chat.rs}`：删除 Runtime waiting task 和事件。
- `apps/cli/src/chat/no_tui.rs`：删除已不存在的 SDK waiting event 分支。
- `apps/cli/src/tui/adapter/tui_runtime_event.rs`：定义 `TuiRunStatus` 并替换字符串 transition。
- `apps/cli/src/tui/adapter/event_mapping.rs`、`event_mapping_tests.rs`、`tui_runtime_event_tests.rs`：SDK typed 状态→TUI DTO 契约。
- `apps/cli/src/tui/model/conversation/run_state.rs`、`run_state_tests.rs`：`RunStateSnapshot`、Main/Sub 身份、幂等和 terminal 保护。
- `apps/cli/src/tui/model/conversation/{intent.rs,intent_impls.rs,model.rs,change.rs,interaction.rs}`：接入 `ObserveRunStatus` 并让旧 lifecycle intent 不再驱动活动展示。
- `apps/cli/src/tui/adapter/agent_event.rs`、`agent_event/tests.rs`：transition→Intent；四类模型活动→瞬时信号。
- `apps/cli/src/tui/view_state/run_activity.rs`、`run_activity_tests.rs`、`view_state.rs`：本地单调静默计时和动画。
- `apps/cli/src/tui/view_assembler/run_activity.rs`、`run_activity_tests.rs`、`live_status.rs`、`output.rs`：typed status→活动行与静默 block。
- `apps/cli/src/tui/view_model/live_status.rs`、`output.rs`：纯 `RunActivityView` 与临时 block DTO。
- `apps/cli/src/tui/render/output_area/{spinner.rs,spinner_tests.rs}`、相关 status-line fixtures：消费新的单帧活动 View。
- `apps/cli/src/tui/model/conversation/{spinner.rs,runtime_state.rs}`、`app/update/spinner.rs` 与调用点：删除第二生命周期字段和写入口。
- `apps/cli/src/tui/architecture_tests.rs`：禁止字符串 Run status、第二状态源和 `ModelStreamWaiting` 回归。
- `apps/cli/src/tui/app/scenario_tests/run_activity.rs`：完整用户场景矩阵。

### Task 1: SDK typed RunStatus Published Language

- [ ] **Step 1: 在 `packages/sdk/src/chat_event_tests.rs` 写失败测试**

定义 `all_run_status_views()`，穷举断言 17 个状态的 `serde_json` snake_case 值，并构造 `ChatEvent::RunTransitioned` 证明 `status` 类型是 `RunStatusView`。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p sdk chat_event::tests::run_status_view_serializes_all_variants -- --exact`
Expected: FAIL，`RunStatusView` 尚不存在。

- [ ] **Step 3: 实现最小 typed PL**

在 `chat_event.rs` 新增 `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]`、`#[serde(rename_all = "snake_case")] enum RunStatusView`，变体与 Runtime 当前 `RunStatus` 一一对应；把 `RunTransitioned.status` 改为该类型，并从 `lib.rs`、`chat.rs` re-export。

- [ ] **Step 4: 运行 SDK 测试**

Run: `cargo test -p sdk chat_event::tests::run_status_view_serializes_all_variants`
Expected: PASS。

- [ ] **Step 5: 提交**

Run: `git add packages/sdk/src/chat_event.rs packages/sdk/src/chat_event_tests.rs packages/sdk/src/lib.rs packages/sdk/src/chat.rs && git commit -m 'feat(sdk): #742 发布 typed RunStatusView'`

### Task 2: Runtime RunStatus 到 SDK 的穷举映射

- [ ] **Step 1: 在 `sdk_event_mapper_tests.rs` 写全变体失败测试**

用表驱动构造每个 `RunStatus` 的 `RunDomainEvent::Transitioned`，断言 `map_domain_event` 返回同名 `RunStatusView` 且 `run_id/parent_run_id` 不丢失。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p runtime adapters::sdk_event_mapper_tests::transitioned_status_maps_every_runtime_variant`
Expected: FAIL，当前 mapper 输出 String。

- [ ] **Step 3: 实现 `run_status_to_sdk`**

在 `sdk_event_mapper.rs` 添加私有穷举函数，逐一映射全部 17 个变体，替换 `format!("{to:?}")`。

- [ ] **Step 4: 运行 Runtime mapper 测试**

Run: `cargo test -p runtime adapters::sdk_event_mapper_tests::transitioned_status_maps_every_runtime_variant`
Expected: PASS。

- [ ] **Step 5: 提交**

Run: `git add agent/features/runtime/src/adapters/sdk_event_mapper.rs agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs && git commit -m 'feat(runtime): #742 穷举发布 Run 状态'`

### Task 3: TUI ACL typed 状态链

- [ ] **Step 1: 写 SDK→TUI DTO 失败测试**

在 `event_mapping_tests.rs` 表驱动所有 `sdk::RunStatusView`，断言 `sdk_event_to_tui_event` 返回 `TuiRuntimeEvent::Run { parent_run_id, event: TuiRunEvent::Transitioned { status: TuiRunStatus } }`，身份和变体一致。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli tui::adapter::event_mapping_tests::run_status_view_maps_without_string_erasure`
Expected: FAIL，TUI transition 仍持 String。

- [ ] **Step 3: 实现 TUI-owned 状态 DTO**

在 `tui_runtime_event.rs` 定义 `TuiRunStatus` 17 变体；`Transitioned.status` 改为该类型；`event_mapping.rs` 添加穷举 `run_status` 转换。

- [ ] **Step 4: 修正日志格式并运行 ACL 测试**

将 typed status 日志改为 `{:?}`，执行：`cargo test -p cli tui::adapter::event_mapping_tests::run_status_view_maps_without_string_erasure`。
Expected: PASS。

- [ ] **Step 5: 提交**

Run: `git add apps/cli/src/tui/adapter/tui_runtime_event.rs apps/cli/src/tui/adapter/event_mapping.rs apps/cli/src/tui/adapter/event_mapping_tests.rs apps/cli/src/tui/effect/session/processing/logging.rs && git commit -m 'feat(tui): #742 typed Run 状态进入 ACL'`

### Task 4: RunStateSnapshot reducer

- [ ] **Step 1: 新建 `run_state_tests.rs` 失败矩阵**

覆盖：未知 Run transition 建立 snapshot；重复通知 no-op；Main 更新 active identity；Sub 不覆盖 Main；terminal 后迟到 live 状态不回滚；新 Main 替换旧 terminal Main。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli tui::model::conversation::run_state_tests`
Expected: FAIL，模块和类型尚不存在。

- [ ] **Step 3: 实现 `run_state.rs`**

定义 `RunStateSnapshot { run_id, parent_run_id, status }`、terminal 判定与 `observe_status`；在 ConversationModel 保存 snapshots 和 active Main identity；新增 `ObserveRunStatus` Intent 与 `RunStatusObserved` Change。

- [ ] **Step 4: 让第二层 ACL 显式消费 transition**

`agent_event.rs` 将 `TuiRunEvent::Transitioned` 映射到 `ObserveRunStatus`，补 mapper 测试；不得返回 default mapping。

- [ ] **Step 5: 运行 Model 与 ACL 测试**

Run: `cargo test -p cli tui::model::conversation::run_state_tests && cargo test -p cli tui::adapter::agent_event::tests::transitioned_run_maps_to_status_observation`
Expected: PASS。

- [ ] **Step 6: 提交**

Run: `git add apps/cli/src/tui/model/conversation apps/cli/src/tui/adapter/agent_event.rs apps/cli/src/tui/adapter/agent_event/tests.rs apps/cli/src/tui/update/root_reducer.rs && git commit -m 'feat(tui): #742 镜像 Runtime Run 状态'`

### Task 5: RunActivityState 与可注入单调时间

- [ ] **Step 1: 新建 `run_activity_tests.rs` 失败测试**

用固定 `Duration`/测试 clock 值覆盖：进入 InvokingModel；9.999 秒不静默；10 秒静默；有效活动重置；离态清理；再次进入重新计时；Sub 事件与无效活动不重置；动画 frame 推进和 verb 稳定。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli tui::view_state::run_activity_tests`
Expected: FAIL，类型尚不存在。

- [ ] **Step 3: 实现 `RunActivityState`**

使用注入的 `now: Instant` 参数，不让类型自行读墙钟；提供 `sync_main_status`、`observe_main_model_activity`、`advance_frame`、`is_model_silent(now)`。

- [ ] **Step 4: 接入现有 SpinnerTick**

将 AppViewState 的 `SpinnerAnim` 切到 `RunActivityState`；Tick 只推进动画和检查静默边界，idle 不重建历史文档。

- [ ] **Step 5: 运行 ViewState 与 Tick 测试**

Run: `cargo test -p cli tui::view_state::run_activity_tests && cargo test -p cli tui::app::update::notice_tests::active_spinner_tick_requests_redraw_without_rebuilding_output`
Expected: PASS。

- [ ] **Step 6: 提交**

Run: `git add apps/cli/src/tui/view_state.rs apps/cli/src/tui/view_state/run_activity.rs apps/cli/src/tui/view_state/run_activity_tests.rs apps/cli/src/tui/app/update.rs apps/cli/src/tui/app/update/notice_tests.rs && git commit -m 'feat(tui): #742 添加 Main Run 活动瞬时态'`

### Task 6: RunActivityView 与活动状态矩阵

- [ ] **Step 1: 写 ViewAssembler 失败矩阵**

在 `run_activity_tests.rs` 覆盖每个 typed status；AwaitingUser/Approval/terminal 无 spinner；ExecutingTools 有/无 tool detail；Compacting 有/无 progress；Sub snapshot 不展示。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli tui::view_assembler::run_activity_tests`
Expected: FAIL，assembler 尚不存在。

- [ ] **Step 3: 实现纯 assembler 和 ViewModel**

新增 `RunActivityKind`、`RunActivityView`；live status 使用 `run_activity: Option<_>`。保留现有 Render 需要的 frame/verb/elapsed 字段，但由新 view 一次性提供，不暴露业务 status。

- [ ] **Step 4: 改 Render 消费单帧 view**

更新 spinner/status-line 渲染与 fixtures，不读取 Model/ViewState；为每类关键文案和无活动状态补 cell 断言。

- [ ] **Step 5: 运行 assembler/render 测试**

Run: `cargo test -p cli tui::view_assembler::run_activity_tests && cargo test -p cli tui::render::output_area::spinner_tests`
Expected: PASS。

- [ ] **Step 6: 提交**

Run: `git add apps/cli/src/tui/view_assembler apps/cli/src/tui/view_model apps/cli/src/tui/render/output_area apps/cli/src/tui/render/output/status_line.rs && git commit -m 'feat(tui): #742 从 Run 状态派生活动展示'`

### Task 7: Main InvokingModel 静默临时 block

- [ ] **Step 1: 写 Output assembler 失败测试**

覆盖 10 秒边界、`Thinking.`/`..`/`...` 动画、稳定 block id、真实活动后消失、再次静默重现、Sub 不影响 Main、占位不进入 timeline/revision/history。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli tui::view_assembler::output_tests::model_silence_placeholder`
Expected: FAIL，output assembler 尚不读 activity state。

- [ ] **Step 3: 实现瞬时 block 组装**

扩展 Output assembler 输入为只读 `RunActivityState`/派生 view，在 roots 尾部追加 `ModelStreamPlaceholderBlockView` 的替代 typed 临时 block；保持同一静默区间 identity 稳定，使用 THINKING semantic。

- [ ] **Step 4: 接入四类有效模型活动**

ACL 对非空 Text/Thinking、ToolCallStart、arguments 实际变化的 ToolCallUpdate 产生显式 Change/信号；App 在 reducer 后更新 activity state。空 delta、Usage、状态、控制、Sub 不更新。

- [ ] **Step 5: 运行 Output 与场景测试**

Run: `cargo test -p cli tui::view_assembler::output_tests::model_silence_placeholder && cargo test -p cli tui::app::scenario_tests::run_activity`
Expected: PASS。

- [ ] **Step 6: 提交**

Run: `git add apps/cli/src/tui/view_assembler apps/cli/src/tui/view_model apps/cli/src/tui/adapter/agent_event.rs apps/cli/src/tui/model/conversation apps/cli/src/tui/app && git commit -m 'feat(tui): #742 派生模型静默占位'`

### Task 8: 删除 ModelStreamWaiting 全链路

- [ ] **Step 1: 先运行现有 waiting 测试建立重构基线**

Run: `cargo test -p runtime stream_handler_tests && cargo test -p cli model_stream_waiting`
Expected: 当前旧测试 PASS；记录需替换的行为。

- [ ] **Step 2: 删除 Runtime waiting task 与事件**

移除 `invocation.rs` 的 waiting task、`StreamProgressSnapshot` 仅服务 waiting 的字段/函数、RuntimeStreamEvent 变体及 SDK mapper 分支。

- [ ] **Step 3: 删除 SDK/no-TUI/TUI waiting 分支**

删除 SDK event、no-TUI match、TUI DTO、legacy UiEvent、placeholder Intent/Model 字段、旧 assembler 分支与测试。

- [ ] **Step 4: 运行反向 grep 与定向测试**

Run: `! rg 'ModelStreamWaiting|should_emit_model_stream_waiting|model_stream_placeholder' agent/features/runtime packages/sdk apps/cli/src`
Expected: exit 0（无匹配）。随后执行 `cargo test -p runtime && cargo test -p sdk && cargo test -p cli`。

- [ ] **Step 5: 提交**

Run: `git add agent/features/runtime packages/sdk apps/cli && git commit -m 'refactor(runtime): #742 删除模型等待专用事件'`

### Task 9: 删除 TUI 第二执行状态源

- [ ] **Step 1: 更新旧 spinner 测试为 typed status 期望**

将 runtime_state、root_reducer、resume、interaction、hook、compact 测试从直接写 `chat_active/phase/counter` 改为发 `ObserveRunStatus` 和 detail intent；先确认因旧字段仍存在而断言不满足目标。

- [ ] **Step 2: 删除旧字段和写入口**

删除 `spinner.rs`、RuntimeState.spinner、`start_chat/complete_chat/generate/think/start_tool_call/complete_tool_call/pause_chat/resume_chat/abort_chat/force_idle/set_spinner_phase/stop_spinner/start_compact` 的生命周期作用；删除 SetSpinnerPhase/StopSpinner Intent、app update helper 与 Hook spinner phase adapter。

- [ ] **Step 3: 清除各内容 Intent 的生命周期副作用**

Text/Thinking/Tool/Hook/Compact/Done 只维护自身事实或 detail；terminal 处理继续由权威 terminal path 完成 processing 和提示，不触碰活动状态。

- [ ] **Step 4: 运行反向 grep**

Run: `! rg 'chat_active|running_tool_count|SetSpinnerPhase|StopSpinner|spinner_phase\(' apps/cli/src/tui`
Expected: exit 0。`SpinnerPhase` 若仍存在仅可作为纯 ViewModel kind；优先完全删除。

- [ ] **Step 5: 运行全部 CLI 测试**

Run: `cargo test -p cli`
Expected: PASS，0 failed。

- [ ] **Step 6: 提交**

Run: `git add apps/cli/src/tui && git commit -m 'refactor(tui): #742 删除 spinner 第二状态源'`

### Task 10: 架构 Guard、文档回填与完整验证

- [ ] **Step 1: 写架构测试**

在 `architecture_tests.rs` 扫描并禁止：`RunTransitioned.status: String`、`format!("{to:?}")` 状态映射、`chat_active`、`running_tool_count`、业务 SpinnerPhase 存储、`ModelStreamWaiting`。

- [ ] **Step 2: 故意违规验证 Guard**

临时在受扫描 fixture/源码加入每类违规，逐条运行对应 architecture test，Expected: FAIL；恢复临时改动后再次运行，Expected: PASS。不得提交故意违规。

- [ ] **Step 3: 回填设计文档与 Issue 差异状态**

将开发前差异逐项标为已对齐/延期，更新 Migration Governance 的实际代码路径与验证证据；Issue 正文 checklist 只勾选有命令证据的条目。

- [ ] **Step 4: 格式与生产编译**

Run: `cargo fmt --check && cargo build -p sdk && cargo build -p runtime && cargo build -p cli`
Expected: 全部 exit 0。

- [ ] **Step 5: 定向与 workspace 测试**

Run: `cargo test -p sdk && cargo test -p runtime && cargo test -p cli && cargo test --workspace`
Expected: 全部 PASS，0 failed；首次失败必须保留并修复，不以重跑覆盖。

- [ ] **Step 6: Clippy 与架构守卫**

Run: `cargo clippy --workspace --all-targets -- -D warnings && bash .agents/hooks/check-architecture-guards.sh`
Expected: exit 0；若脚本名称/入口与仓库实际不同，使用 `.agents/aemeath.json` 注册的 Stop guard 总编排并在 PR Test plan 记录精确命令。

- [ ] **Step 7: 最终范围核对和提交**

Run: `git diff origin/main...HEAD --check && git status --short && git log --oneline origin/main..HEAD`
Expected: 无未提交改动、无 diff whitespace error；提交最终文档/Guard：`git commit -am 'test(tui): #742 固化 Run 活动单一真相守卫'`。
