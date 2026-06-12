<!-- Migrated from: docs/feature/active.md#69 -->
### #69 TUI Hook 消息类型化与 system-reminder 展示脱壳

**状态**：待确认

**背景**：Stop hook 阻止结束时，反馈既需要作为 `<system-reminder>` 注入下一轮 LLM 上下文，也需要在 TUI 中提示用户。但当前用户可见提示沿用普通 `SystemMessage`/`SystemNotice` 展示，视觉上接近用户输入；部分场景还可能把 `<system-reminder>` 标签原样展示，造成用户误以为系统内部标签进入了可见对话内容。后续 StopFailure 或其他 hook 也可能产生不同语义的用户可见消息，继续复用普通 SystemMessage 会让样式、文案和脱壳规则分散。

**目标**：
1. TUI 展示层对 `<system-reminder>...</system-reminder>` 包装做统一脱壳，只展示内部的人类可读内容。
2. 新增 Hook 类用户可见消息，避免 Hook 反馈混用普通 SystemNotice；Hook 消息应能承载来源事件或语义类型，便于 Stop、StopFailure 和未来 Hook 使用不同文案/样式。
3. 保留 LLM 上下文注入中的 `<system-reminder>` 语义，不改变模型可见系统提醒协议；脱壳仅作用于 TUI 可见展示。
4. Hook 阻止类消息在视觉上应与用户输入明显区分，优先使用 warning/error 语义色或明确前缀。

**建议实现方向**：
1. 已在 SDK/runtime 事件层引入统一 `HookEvent`/`HookEventStatus`，用 `Running`、`Succeeded`、`Blocked`、`Failed` 表达 hook 生命周期和结果。
2. 已移除旧 `HookStart`、`HookEnd`、`StopFailureHook` 事件路径；所有 hook 执行统一发送 `HookEvent`。
3. 已在 TUI adapter/model 层新增 `ConversationBlock::HookNotice` 与 `OutputBlockKind::HookNotice`，由 TUI 根据 `HookEvent` 派生 blocked/failed notice 文案和 warning/error 样式。
4. 已抽出单一 helper 剥离完整包裹的 `<system-reminder>` 标签，并应用到 TUI 可见 SystemNotice/HookNotice 展示。
5. Stop hook blocked 不再发送普通 `SystemMessage` 作为用户提示，但仍保留返回给 loop 的 feedback，用于继续注入 LLM 上下文。

**验收标准**：
1. 当用户可见消息文本为完整 `<system-reminder>...</system-reminder>` 包装时，TUI 输出区不显示开始/结束标签。
2. Hook 反馈以 Hook notice 类型进入 conversation/view model，不再只依赖普通 SystemNotice。
3. Stop hook blocked 提示仍会显示命令和失败详情；长输出写入文件路径等信息不丢失。
4. LLM messages 中用于继续对话的 `<system-reminder>` 包装保持不变。
5. 单元测试覆盖：脱壳正常路径、无标签边界、标签不完整/嵌入普通文本时不误删，以及 Hook notice 的样式/类型映射。

**明确不做**：
1. 不重做所有 SystemNotice 的视觉设计；本 feature 只处理 Hook 类消息和 system-reminder 脱壳。
2. 不改变 Hook 执行协议、JSON schema 或阻止逻辑。
3. 不把所有 LLM system reminder 从消息历史中移除；仅区分模型上下文与 TUI 展示。

**验证**：
- `cargo check`（baseline）
- `cargo check -p runtime -p sdk`
- `cargo check -p cli`
- `cargo test -p cli hook_notice --bins`
- `cargo test -p cli system_reminder --bins`
- `cargo test -p runtime stop_hook --lib`

**涉及路径（预计）**：
- `agent/features/runtime/src/business/chat/looping/finalize.rs`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- `packages/sdk/src/*`（如需新增 ChatEvent 类型）
- `apps/cli/src/tui/effect/session/processing.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/*`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/render/output/blocks/diagnostic.rs` 或新增 Hook notice renderer
