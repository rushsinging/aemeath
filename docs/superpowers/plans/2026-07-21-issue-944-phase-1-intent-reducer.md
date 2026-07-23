# Issue #944 阶段一实施计划：Intent 与 Root Reducer 骨架

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)，父 Issue：#860。
> 基线：`refactor/944-tui-tea-core`，阶段一只建立迁移入口，不改变既有事件桥或运行时行为。

## 目标

建立可测试的 `AgentIntent → root_reducer::reduce_intent → TuiUpdateResult` 新入口，覆盖现有 Conversation、Input、Diagnostic、Session 四个 Context。该入口复用已有 Context `apply()` 与 Conversation Change→runtime/dirty 归并逻辑；不触及 Config/Workspace 的正式拆分。

## 范围与非目标

- 新增 `update/intent.rs`：闭合 `AgentIntent` 枚举，按 Context 标记现有 Intent。
- 扩展 `update/root_reducer.rs`：新增 `reduce_intent`，统一派发四类 Intent、归并 dirty、只产生一个 `RequestRender`。
- 为 root reducer 添加最小 Context mutation façade；本阶段字段仍为 public，以保持全部既有调用点兼容。
- 新增 root reducer 单元测试：Conversation 与 Input 各一条；附带 Diagnostic/Session 分发覆盖。
- **不迁移** App、effect、render、`update_ui` 或 `AgentEventMapping`；不删除旧 `.apply()` 调用；不引入 Config/Workspace intent；不私有化字段。

## 冻结规则

阶段一完成后，新增生产 mutation MUST 使用 `AgentIntent + reduce_intent`，不得新增 `model.{conversation,input,diagnostic,session}.apply(...)` 调用。本阶段不对存量调用点设零容忍 Guard；阶段三迁移完消费者后再启用零调用 Guard。

## 任务

### Task 1：定义 AgentIntent（TDD）

**文件：**
- Create: `apps/cli/src/tui/update/intent.rs`
- Modify: `apps/cli/src/tui/update.rs`
- Test: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`

1. 先写 reducer 调用 `AgentIntent::Conversation(StartChat)` 的失败测试。
2. 运行 `cargo test -p cli tui::update::root_reducer::intent_tests::conversation_intent_starts_chat_and_marks_output_dirty -- --nocapture`，确认因类型/函数缺失失败。
3. 定义 `AgentIntent::{Conversation, Input, Diagnostic, Session}`，均包装既有 Context intent。
4. 在 `update.rs` 注册 `pub mod intent`。

### Task 2：实现 Root Reducer 新入口（TDD）

**文件：**
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
- Modify: `apps/cli/src/tui/model/root.rs`
- Test: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`

1. 先补 Input、Diagnostic、Session 的失败测试：分别断言 Context 状态更新和正确 dirty 位。
2. 实现 `reduce_intent(model, AgentIntent) -> TuiUpdateResult`：
   - Conversation 复用 `apply_conversation_changes`；
   - Input 标记 `dirty.input`；
   - Diagnostic 标记 `dirty.status + dirty.dialog`；
   - Session 标记 `dirty.status`；
   - 统一去重并按任意 dirty 插入单一 `Effect::RequestRender`。
3. TuiModel 增加仅供 reducer 调用的 crate-private façade，避免 reducer 后续继续依赖公开字段；本阶段不收紧字段可见性。
4. 执行 `cargo test -p cli tui::update::root_reducer::intent_tests -- --nocapture`。

### Task 3：阶段一验证与退役门槛

**文件：**
- Modify: `apps/cli/src/tui/architecture_tests.rs`

1. 增加 L0 静态测试，仅断言 `AgentIntent` 四 Context 变体及 `reduce_intent` 入口存在；不得扫描/阻断存量直接 apply。
2. 执行：
   - `cargo test -p cli tui::architecture_tests -- --nocapture`
   - `cargo test -p cli tui::update::root_reducer -- --nocapture`
   - `cargo fmt --all -- --check`
   - `cargo check -p cli`
   - `git diff --check`
3. 阶段一无可删除旧路径。阶段二的显式退役目标：`update/spinner.rs` 的直接 spinner 字段写入、`RuntimeState::spinner_mut()`、queued/compact 旁路 mutation。

## 完成定义

- 四 Context 均能通过 `AgentIntent` 进入 root reducer；
- Conversation/Input/Diagnostic/Session 的 L1 断言通过；
- 每次 `reduce_intent` 最多产生一个 `RequestRender`；
- 现有 `reduce_agent_event` 行为和存量调用点保持不变；
- 阶段二开始前，不新增旧 Context 直接 apply 调用。
