# Bug #114 Stop Hook Blocking + Chat Loop FSM — 规格

**日期**：2026-06-03
**Bug**：#114
**状态**：设计中

## 背景

Bug #114 当前记录将 Stop hook blocking 描述为“LLM 已发送完所有输出，hook 拦截无法中止已完成响应”。用户确认该表述需要修正：Stop hook blocked 的语义不是撤销已经流式显示的 token，而是**阻止 chat loop 真正停止**。当 Stop hook 返回 blocked 时，runtime MUST 告知 LLM 当前不能结束，必须满足 Stop hook 要求后才能再次尝试停止。

现有 `process_chat_loop` 已通过 `continue` 隐式实现了部分语义：LLM 结束后运行 Stop hook；若 hook blocked，则追加 system reminder 并进入下一轮。但宏观状态全部散落在分支和 `continue/break` 中，缺少显式状态表达，导致代码可读性、测试性和用户理解都较弱。

## 目标

1. Stop hook blocked MUST 阻止 chat loop 真正停止。
2. Stop hook blocked MUST 将 hook 要求反馈给 LLM，且反馈文案 MUST 明确：LLM 不能结束，必须先满足 Stop hook 要求，再尝试停止。
3. Stop hook success 后，chat loop 才能进入真正完成状态并发送最终 Done/DoneWithDuration 事件。
4. chat loop 的宏观状态 MUST 显式化为轻量手写 FSM，位于 runtime business/core 代码内，NEVER 引入状态图引擎或框架。
5. FSM MUST 与现有 Loop Gate / `GateDecision::{Proceed, ContinueNextTurn, AbortCurrentLoop, CancelCurrentLoop}` 自洽。
6. 实现 MUST 保持现有 provider stream、tool 执行、hook runner、TUI event 协议的主要架构不变。

## 非目标

1. 不实现“stream 生成中途执行 Stop hook 并取消 provider stream”。
2. 不撤销或隐藏已经流式显示给 TUI 的 LLM token。
3. 不重写 Loop Gate、工具执行并发、provider stream handler 或 TUI 渲染架构。
4. 不引入外部 FSM 框架。
5. 不改变 StopFailure hook、PreToolUse/PostToolUse hook 的语义。

## 语义定义

### “阻止停止”

Stop hook blocked 表示：当前 assistant 已尝试完成本轮处理，但 runtime 不允许 chat loop 进入最终 Done 状态。runtime MUST 把 blocked 反馈注入为下一轮 LLM 可见上下文，并继续 loop。

### “真正停止”

只有满足以下条件时，chat loop 才算真正停止：

1. LLM 尝试结束；
2. BeforeFinish Loop Gate 没有要求继续下一轮；
3. Stop hook 执行且没有 blocked；
4. Stop hook 后再次经过必要的 BeforeFinish Gate；
5. runtime 发送 `DoneWithDuration` 或对应最终事件并 break。

## 宏观 FSM

新增轻量状态机，建议位置：

```text
agent/features/runtime/src/business/chat/looping/state.rs
```

状态集合：

```text
Running
AwaitingTool
AwaitingUser
Compacting
Stopping
StopHookBlocked
Done
```

推荐转移：

```text
StartTurn / ResumeRunning -> Running
Running -> Compacting -> Running
Running -> AwaitingTool -> AwaitingUser -> Running
Running -> AwaitingUser -> Running
Running -> Stopping
Stopping -> StopHookBlocked -> Running
Stopping -> Done
Any -> Done on AbortCurrentLoop / CancelCurrentLoop
```

FSM 是领域辅助对象，不拥有 I/O，不直接发送 TUI 事件。它 SHOULD 只负责转移、记录当前状态，并可输出 debug 日志。

## Loop Gate 对接规则

1. BeforeLlm Gate：
   - `Proceed`：保持/进入 `Running`。
   - `ContinueNextTurn`：保持/进入 `Running`，使用追加后的 messages 调用 LLM。
   - `AbortCurrentLoop` / `CancelCurrentLoop`：进入 `Done`，不得调用 LLM。

2. AfterBlockingBoundary Gate：
   - 工具或 hook 边界后进入 `AwaitingUser`；若有 user message 则转回 `Running` 并继续下一轮。
   - Abort/Cancel 进入 `Done`。

3. BeforeFinish Gate：
   - 调用前进入 `AwaitingUser` 或 `Stopping` 的前置检查阶段。
   - `ContinueNextTurn` MUST 阻止最终 Done，转回 `Running`。
   - Abort/Cancel 进入 `Done`。

4. Stop hook：
   - 运行前 MUST 转入 `Stopping`。
   - blocked MUST 转入 `StopHookBlocked`，追加 system reminder，然后 `ResumeRunning`。
   - success MUST 转入 `Done`，之后才发送 `DoneWithDuration`。

## Stop hook blocked 反馈文案

反馈给 LLM 的 system reminder MUST 包含强约束语义。建议中文主文案：

```text
Stop hook 阻止了停止。你现在还不能结束本轮处理。
你 MUST 先满足下面 Stop hook 的要求，然后才能再次尝试停止。
```

随后 MUST 包含：

1. hook 命令；
2. JSON reason / stderr / stdout / 长输出文件路径等现有诊断内容；
3. 若输出过长，继续使用现有临时文件保存策略。

## 测试要求

1. FSM 单元测试：
   - `Running -> Stopping -> StopHookBlocked -> Running -> Stopping -> Done`。
   - Tool/User 边界：`Running -> AwaitingTool -> AwaitingUser -> Running`。
   - Abort/Cancel 任意状态进入 `Done`。

2. Stop hook feedback 单元测试：
   - blocked 文案包含“不能结束”或等价强约束；
   - blocked 文案包含 `MUST` 或中文强制语义；
   - 长输出文件策略保持不变。

3. Chat loop 行为测试（若现有 harness 支持）：
   - 第一次 Stop hook blocked 时不发送 `DoneWithDuration`；
   - blocked 后追加 system reminder 并进入下一轮；
   - 后续 Stop hook success 后才发送 `DoneWithDuration`。

4. 验证命令：
   - `cargo test -p runtime chat_loop_state`
   - `cargo test -p runtime stop_hook_feedback`
   - `cargo test -p runtime stop_hook`
   - `AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh`
   - 必要时运行 `.agents/hooks/check-unit-tests.sh`

## Bug 追踪更新

Bug #114 的 active 记录 MUST 从“Stop hook 阻止停止无意义”更新为：

```text
Stop hook blocked 缺少显式 chat loop 停止状态表达
```

根因类别应说明：Stop hook blocked 的控制流依赖隐式 `continue`，缺少显式 FSM 和强约束反馈文案，导致用户误解为 hook 在“已完成后无意义阻止”。

## 验收标准

1. 代码中存在轻量 `ChatLoopFsm` 或等价类型，且有单测覆盖关键转移。
2. Stop hook blocked 仍会阻止最终 Done。
3. Stop hook blocked 反馈明确告知 LLM 不能结束，必须先满足 hook 要求。
4. Stop hook success 后才发送最终完成事件。
5. 验证命令通过。
