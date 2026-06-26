# Rust 大文件拆分计划

> 对应 Issue: 待创建
> 分支: `refactor/file-split`

## 背景

项目有 616 个 Rust 文件、共 90,507 行。其中有大量 400+ 行的大文件，不利于 LLM 读取和上下文管理。本计划对超过 350 行的非测试文件进行结构性拆分，目标是将绝大多数文件控制在 350 行以内。

## 拆分原则

1. **零功能变更**：纯结构性重构，不修改任何业务逻辑
2. **保持 `pub` 接口不变**：通过 `mod.rs` + `pub use` 重导出，不破坏外部引用
3. **拆分维度优先级**：职责/关注点 > 类型域 > 方法分组
4. **测试同步迁移**：主文件中的 `mod tests` 提取到 `*_tests.rs`，或随对应实现迁移
5. **每个 Phase 完成后必须通过验证门禁**：`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
6. **不手动调整格式**：格式化由 `cargo fmt` 处理

## 目标行数

| 类型 | 目标 | 理由 |
|---|---|---|
| 核心逻辑文件 | ≤350 行 | LLM 单次可完整读取，聚焦单一职责 |
| 纯函数/无状态文件 | ≤400 行 | 可读性尚可，可接受 |
| 测试文件 | ≤500 行 | 超过时按域拆分 |

## 分阶段计划

### Phase 0：测试提取（零功能风险）

将内联 `mod tests` 提取到独立的 `*_tests.rs` 文件，仅减少主文件行数，不改变实现文件结构。

| # | 文件 | 当前行数 | 策略 | 目标行数 |
|---|---|---|---|---|
| 0.1 | `agent/features/runtime/src/business/chat/looping/reflection.rs` | 556 | 测试 ~425 行提取 → `reflection_tests.rs` | ~130 |
| 0.2 | `agent/features/runtime/src/business/chat/looping/finalize.rs` | 505 | 测试 ~140 行提取 → `finalize_tests.rs` | ~360 |
| 0.3 | `apps/cli/src/tui/model/input/document.rs` | 688 | 测试 ~360 行提取 → `document_tests.rs` | ~330 |
| 0.4 | `apps/cli/src/tui/model/input/model.rs` | 592 | 测试 ~280 行提取 → `input_model_tests.rs` | ~310 |
| 0.5 | `apps/cli/src/tui/render/output_area/render.rs` | 560 | 测试 ~260 行提取 → `render_tests.rs` | ~300 |
| 0.6 | `apps/cli/src/tui/model/conversation/model.rs` | 564 | 无内联测试，跳过（Phase 5 处理） | — |
| 0.7 | `apps/cli/src/tui/view_state/output.rs` | 540 | 测试提取 → `output_tests.rs` | ~200 |
| 0.8 | `apps/cli/src/tui/view_assembler/output.rs` | 540 | 测试提取 → `output_tests.rs` | ~200 |
| 0.9 | `agent/features/project/src/business/workspace_state.rs` | 467 | 测试 ~260 行提取 → `workspace_state_tests.rs` | ~200 |
| 0.10 | `packages/global/logging/src/unified_logger.rs` | 465 | 测试提取 → `unified_logger_tests.rs` | ~320 |
| 0.11 | `apps/cli/src/tui/app/update/notice.rs` | 463 | 测试提取 → `notice_tests.rs` | ~200 |
| 0.12 | `agent/features/tools/src/business/web_search.rs` | 408 | 测试已在 `mod tests;` → 检查是否需拆分 `tests/` 子模块 | ~400 |
| 0.13 | `agent/features/tools/src/business/file_edit.rs` | 392 | 测试 ~70 行 → `file_edit_tests.rs` | ~320 |

**验证**：`cargo test` 全量通过。

---

### Phase 1：`config_manager.rs` patch 拆分

**文件**：`agent/features/runtime/src/utils/bootstrap/config_manager.rs`（1,154 行）

**问题**：`impl ConfigManager` 包含 25 个 `apply_xxx_patch` 方法，占 ~500 行。

**拆分方案**：
```
bootstrap/
├── config_manager.rs       ~500行：ConfigManager 结构 + new/load/save/update/get + apply_env_vars
└── config_patch.rs         ~350行：apply_patch + 全部 apply_xxx_patch 系列方法
```

**具体操作**：
1. 将 `fn apply_patch` 及所有 `apply_*_patch` / `merge_hooks` 移到 `config_patch.rs`
2. `config_manager.rs` 中 `use crate::utils::bootstrap::config_patch::apply_patch;`
3. 在 `bootstrap/mod.rs`（或 `lib.rs`）注册新模块
4. `pub(crate)` 可见性保持不变

**验证**：`cargo test -p aemeath-runtime`。

---

### Phase 2：`trait_command.rs` 按命令域拆分

**文件**：`agent/features/runtime/src/core/client/trait_command.rs`（905 行）

**问题**：18 个 `*_impl` 命令函数 + reflection/memory 辅助函数全堆一个文件。

**拆分方案**：
```
client/
├── trait_command.rs       ~150行：execute_command_impl + estimate_context_impl
├── trait_model.rs         ~120行：switch_model_impl + set_thinking_impl + list_models_impl + switch_model_openai_config
├── trait_reflection.rs    ~250行：run_reflection_impl + apply_reflection_impl + reflection/memory 辅助函数
├── trait_memory.rs        ~50行：list_reminders_impl + add_reminder_impl + current_timestamp_secs
├── trait_compact.rs       ~40行：compact_messages_impl + compact_impl
└── trait_misc.rs          ~60行：notify_hook_impl + read_clipboard_image_impl + process_image_file_impl
```

**可见性处理**：
- 所有 `*_impl` 函数从 `pub(super)` 保持在 `client` 模块内
- 在 `client/mod.rs`（或当前模块声明处）注册新子模块

**验证**：`cargo test -p aemeath-runtime`。

---

### Phase 3：`chat.rs`（sdk）类型域拆分

**文件**：`packages/sdk/src/chat.rs`（601 行）

**问题**：20+ 类型定义集中，涵盖输入、事件、视图模型、结果等不同域。

**拆分方案**：
```
sdk/src/
├── chat.rs                ~90行：pub use 重导出 + ChatInput + ChatRequest + ChatInputEvent
├── chat_event.rs          ~200行：ChatEvent + ChatEventContext + ToolCallStatusView + Result/Done 事件
├── chat_view.rs           ~150行：AgentProgress*View + AgentProgressKindView + Hook*View + Workspace*View + OptionItem
└── chat_result.rs         ~60行：ChatResult + ChatStream + ToolResultImage
```

**公共 API 保持**：`chat.rs` 中通过 `pub use` 重导出所有类型，确保 `sdk::ChatEvent` 等路径不变。

**验证**：`cargo test --workspace`（sdk 是跨 crate 公共接口）。

---

### Phase 4：`loop_runner.rs` 状态机拆分

**文件**：`agent/features/runtime/src/business/chat/looping/loop_runner.rs`（1,219 行）

**问题**：`process_chat_loop()` 是一个 ~580 行的巨型 async 函数，内含多阶段循环逻辑。

**拆分策略**：先读取完整代码，识别自然循环阶段边界，再拆分。

**初始方案**（待代码确认后调整）：
```
looping/
├── loop_runner.rs         ~250行：ChatLoopContext + process_chat_loop 主调度
├── loop_helpers.rs        ~50行：chat_loop_transition_for_gate_exit + is_user_cancelled_provider_error + drain_and_apply_gate
└── loop_phases.rs         ~350行：从 process_chat_loop 中提取的阶段处理函数
```

**风险**：这是 Agent 核心循环，改动最复杂。需特别小心：
- async 函数中的 borrow checker 约束
- 跨 await 点的状态所有权
- `process_chat_loop` 的泛型参数 `<S, Q, I>`

**验证**：`cargo test -p aemeath-runtime`，重点关注 loop 相关测试。

---

### Phase 5：TUI 文件按职责拆分

| # | 文件 | 当前行数 | 拆分方案 |
|---|---|---|---|
| 5.1 | `effect/session/processing.rs` | 799 | → `processing.rs`(~180行：端口实现+spawn) + `event_mapping.rs`(~160行：sdk_event_to_ui_event) + `event_logging.rs`(~210行：log 函数) |
| 5.2 | `render/display/render.rs` | 584 | → `render.rs`(~170行：App render) + `history_parse.rs`(~210行：parse_history_*) |
| 5.3 | `model/conversation/model.rs` | 564 | → `model.rs`(~200行：ConversationModel + apply) + `observer.rs`(~220行：observe_tool_call_* + append/extend) |
| 5.4 | `render/output_area/render.rs` | 560 | → `render.rs`(~170行) + `render_helpers.rs`(~90行：sel_range/clear_area/scrollbar) — 测试已在 Phase 0 提取 |
| 5.5 | `update/root_reducer.rs` | 506 | 按消息域拆分 reducer |
| 5.6 | `render/output/blocks/ask_user.rs` | 487 | 按渲染/交互拆分 |
| 5.7 | `render/output/document_renderer.rs` | 494 | 测试提取 |
| 5.8 | `adapter/tool_flow_projector.rs` | 424 | 测试提取 |
| 5.9 | `adapter/agent_event.rs` | 405 | 测试提取 |
| 5.10 | `app/update.rs` | 405 | 按消息域拆分 |
| 5.11 | `app/update/ui_event.rs` | 384 | 测试提取 |
| 5.12 | `app/update/key.rs` | 359 | 测试提取 |
| 5.13 | `app/slash.rs` | 384 | 测试提取 |
| 5.14 | `render/output/blocks/tool_result.rs` | 397 | 测试提取 |
| 5.15 | `render/output/tool_display/tool_impls.rs` | 416 | 按工具类型拆分 |
| 5.16 | `core/client/event.rs` | 435 | 按 SDK/TUI mapping 拆分 |

---

### Phase 6：其余 400–500 行 feature 文件

| # | 文件 | 当前行数 | 拆分方案 |
|---|---|---|---|
| 6.1 | `prompt/guidance/resolver.rs` | 453 | 测试 ~30 行提取；sync/async 分离 |
| 6.2 | `provider/core/client.rs` | 410 | 辅助函数提取 → `client_helpers.rs` |
| 6.3 | `provider/openai_compatible/stream.rs` | 453 | `json_recovery_tests` 已分离；`parse_openai_stream` ~300 行可按阶段提取 |
| 6.4 | `provider/openai_compatible/request_body.rs` | 381 | 测试提取 |
| 6.5 | `runtime/utils/bootstrap/provider_client.rs` | 445 | 测试提取 |
| 6.6 | `runtime/business/state.rs` | 371 | InternalSession + AppState 分离 |
| 6.7 | `runtime/core/client/mapping.rs` | 387 | task_status_lines 提取 → `mapping/task_status.rs` |
| 6.8 | `tools/bash/safety.rs` | 393 | 测试提取 |
| 6.9 | `tools/agent_tool.rs` | 369 | scope 分析提取 → `scope.rs` |
| 6.10 | `hook/hook/runner.rs` | 383 | execute_hook 内部逻辑提取 |
| 6.11 | `tools/mcp_manager/connection.rs` | 381 | 测试提取 |
| 6.12 | `shared/error.rs` | 362 | ErrorDisplay 提取 → `error_display.rs` |
| 6.13 | `runtime/business/compact/token_estimation.rs` | 371 | 测试提取 |
| 6.14 | `runtime/business/compact/summary.rs` | 386 | 测试提取 |
| 6.15 | `runtime/business/prompt/build/prompt_build.rs` | 384 | 测试提取 |
| 6.16 | `storage/task/display.rs` | 385 | 测试提取 |

---

### Phase 7：`ids.rs` 宏化去重

**文件**：`packages/sdk/src/ids.rs`（464 行）

**问题**：`ChatId`、`ChatTurnId`、`ToolCallId` 三个类型的 `impl` 块几乎完全相同（PartialEq/Eq/Hash/Display/AsRef/Serialize，各 ~40 行 × 3 = ~120 行重复）。

**方案**：声明 `macro_rules! impl_id_type` 统一生成 trait impl，消除 ~80 行重复代码。

---

## 执行顺序

```
Phase 0（测试提取）
    ↓
Phase 1（config_manager patch）
    ↓
Phase 2（trait_command 域拆分）
    ↓
Phase 3（sdk chat 类型域）
    ↓
Phase 4（loop_runner 状态机）← 最复杂
    ↓
Phase 5（TUI 文件）
    ↓
Phase 6（其余 feature 文件）
    ↓
Phase 7（ids 宏化）
    ↓
全量验证 + PR
```

每个 Phase 完成后执行验证门禁，确保不引入回归。所有 Phase 在同一分支 `refactor/file-split` 上累积，最终统一创建 PR。

## 风险与缓解

| 风险 | 缓解措施 |
|---|---|
| Phase 4 loop_runner 拆分可能引入行为变化 | 最后执行；逐函数提取；保留原始 git diff 用于对比 |
| Phase 3 sdk 拆分影响跨 crate 公共接口 | `pub use` 保持路径不变；全 workspace 编译验证 |
| 测试提取后私有函数可见性变化 | 使用 `#[cfg(test)]` + `pub(super)` / `pub(crate)` 确保可访问 |
| 重构后代码格式不一致 | 每个 Phase 后 `cargo fmt`，不手动调整 |
