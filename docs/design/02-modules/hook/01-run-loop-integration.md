# Hook · Run Loop Engine 集成

> 层级：02-modules / hook（跨模块协作）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> 本文定义 Stop Hook 与 Run Finishing 的协作；Hook 不拥有 Run 状态机。

## 1. 两层重试必须分离

### Hook 执行重试

Hook BC 内部处理单个命令的瞬时执行故障：

```text
max_attempts = 3
```

该计数回答“同一条 subscription 的命令执行了几次”。

### Stop 阻断循环

Runtime Loop Engine 处理模型再次尝试结束时 Stop Hook 仍未放行：

```text
max_stop_hook_blocks = 15
```

该计数回答“同一个 Run 有多少次停止尝试被 Stop Hook 阻止”。两者不得复用一个 counter。

## 2. Run 状态迁移

```text
ApplyingResponse
  │ 无 ToolCall / EndTurn
  ▼
Finishing
  │
  ├─ HookPort.dispatch(Stop) → Continue
  │      └─ Completed
  │
  └─ HookPort.dispatch(Stop) → Block
         ├─ stop_block_count += 1
         ├─ 未超过 15：追加 system-generated feedback
         │              Finishing → PreparingContext
         │              同一个 Run 继续
         └─ 超过 15：状态 Failed(StopHookRetryExhausted)
                      发布 RunFailed { error: StopHookRetryExhausted }
```

Stop Hook 执行失败时，Hook BC 先尝试最多 3 次；全部失败后合成 `Block(StopHookExecutionFailed)`，再进入同一 Runtime 分支。

## 3. Run 状态迁移表

| 当前态 | 条件 | 下一态 | 所有者 |
|---|---|---|---|
| Finishing | Stop directive=Continue | Completed | Runtime |
| Finishing | Stop directive=Block 且 count≤15 | PreparingContext | Runtime |
| Finishing | Stop ExecutionFailed 重试耗尽且 count≤15 | PreparingContext | Runtime |
| Finishing | Stop block count>15 | Failed(StopHookRetryExhausted) | Runtime |

### Hook 内部重试表

| Hook 内部条件 | Hook 行为 |
|---|---|
| 执行故障且 attempt<3 | 保持同一 dispatch，重试命令 |
| Stop 执行故障且 attempt=3 | 返回 Block(StopHookExecutionFailed)，再尽力派发一次 StopFailure 观察事件 |
| 普通 Hook 执行故障且 attempt=3 | 返回 Continue，并保留 ExecutionFailed 明细 |

Hook 执行重试不是 Run 状态迁移；Run 只观察最终 HookOutcome。

## 4. Counter 生命周期

`stop_block_count`：

- 属于 Run 内存态；
- Run 创建时为 0；
- 主动 Block 与 Stop 执行失败 Block 都递增；
- 普通 model/tool step 不清零；
- Stop Continue 后 Run 结束；
- 不持久化，崩溃后新 Run 从头开始。

`max_stop_hook_blocks=15` 的默认值由 ConfigSnapshot 提供，Runtime 应用；用户 HookSubscription 不能覆盖该上限。

## 5. Feedback

Block feedback 是 system-generated input，至少包含：

- Hook 主动给出的 reason，或执行失败摘要；
- 当前阻断次数 / 最大次数；
- 明确要求模型在下一次停止前满足的条件；
- 不包含原始环境变量、密钥或无限制 stdout/stderr。

Feedback 经 Context Management 进入下一步 Context Window；Hook BC 只返回结构化 reason，不直接修改消息历史。

## 6. Main 与 Sub

### Main Run

超过 15 次后进入 `Failed(StopHookRetryExhausted)`；外层 Session Run 序列仍可等待用户新输入并创建新 Run。

### Sub Run

超过 15 次后进入 `Failed(StopHookRetryExhausted)`；终态事件 `RunFailed { error: StopHookRetryExhausted }` 回传父 Run。Sub 不绕过 Stop，也不自动降级成 Completed。

Main/Sub 使用同一 Loop Engine 和计数规则，不因交付层是否存在而分叉。

## 7. 与 StuckGuard 的关系

Stop block count 是确定性的协议上限，不并入通用 StuckGuard 计分：

- StuckGuard 检测重复文本、工具循环与 wall-clock；
- stop_block_count 检测 Stop 协议无法收敛；
- 两者可产生不同 RunFailed reason；
- stall 导致尝试结束时仍必须经过 Stop Hook，但不能绕过 15 次上限。

## 8. 目标约束

- Stop Hook 的阻断上限必须由 Runtime 配置并守护；
- Hook 执行重试必须由 Hook BC 配置并守护；
- 超过上限只允许进入 RunFailed，不允许把未获放行的 Run 标记为 Completed；
- Main/Sub 必须走同一 Stop Hook 路径；
- 实现差距统一记录在 Migration Governance。

## 9. 验收场景

- [ ] Stop 第 1 次主动 Block，feedback 进入下一步，RunId 不变。
- [ ] Stop 执行故障前两次失败、第 3 次 Continue，Run Completed。
- [ ] Stop 执行连续 3 次失败，合成 Block 并继续 Run。
- [ ] 第 15 次 Block 仍继续；第 16 次进入 `Failed(StopHookRetryExhausted)`。
- [ ] 终态事件为 `RunFailed { error: StopHookRetryExhausted }`，且不发送 RunCompleted。
- [ ] Main 失败后可由新用户输入创建新 Run。
- [ ] Sub 失败终态回传父 Run。
- [ ] cancellation 能终止 Hook 子进程及重试等待。
- [ ] 普通 Hook 执行失败重试耗尽后：未配置 failure_policy → Continue；配置 failure_policy=Block → Block。

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Hook 3 次执行重试、Stop 15 次阻断上限及 RunFailed 语义 | #790 |
