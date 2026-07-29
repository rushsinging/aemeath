# 所有 Tool 硬超时、取消与未完成调用持久化修复设计

> 对应 Issue：[#1440](https://github.com/rushsinging/aemeath/issues/1440)  
> Milestone：v0.1.0 — Context Engineering + 架构重构  
> 状态：修复前设计基线；实现必须遵循 TDD 与逐层验证

## 1. 问题与证据

Session `019fa6cc-9fe5-7dad-992b-f70f01cc38f0` 中曾在 TUI 展示仓库级 `Glob(pattern="**/archify.mjs")`。该调用长时间不返回，用户发送“别find **/archify.mjs 太慢了”并尝试取消；随后 TUI 于 `2026-07-28T12:02:16.957654+08:00` 记录 `auto-save timed out, forcing abort`。

恢复后的 canonical session 保存了用户的停止指令，却没有精确 `**/archify.mjs` 的 `tool_use`、`tool_result` 或稳定 call identity。同一文件中的 `**/bin/archify.mjs` 是更早、已经完成的另一条调用，不是本次丢失调用。

这暴露两个相互放大的缺陷：

1. Runtime 的 `tokio::time::timeout` 只能在 executor 能继续调度 timer 时生效；同步遍历、阻塞 FFI 或不可让出的 Tool 工作若直接运行在 Tokio worker 上，会让 120 秒 deadline 与用户取消都无法及时收敛。
2. Runtime 只在 Step finalized 后把 `step_messages.outcome()` 提交给 Context；调用开始与运行中状态没有 durable checkpoint，强制 abort 会让已经展示/派发的调用永久消失。

## 2. 当前实现差异

| 目标契约 | 当前实现 | 风险 |
|---|---|---|
| 所有 Tool 共享唯一 Runtime deadline 监督器 | `Agent::execute_call`、Bash、Agent/MCP 等路径分别处理 timeout | 新入口可绕过；终态不一致 |
| 120 秒为墙钟硬截止 | 外层 `tokio::time::timeout` 包裹可能阻塞 worker 的 future | timer 无法调度，取消失效 |
| cooperative/non-cooperative 声明影响执行策略 | Descriptor 有声明，执行主链未闭环消费 | 无法证明取消是否真正停止底层工作 |
| 已展示/已派发调用可恢复 | 仅 finalized Step 提交消息 | 中断窗口内调用丢失 |
| terminal receipt 记录真实取消结果 | `persist_step()` 固定传 `vec![]` receipts | `CancellationUnconfirmed` 与 unfinished IDs 未进入生产数据 |
| 日志可重建工具生命周期 | 缺少稳定 call identity 的 start/deadline/cancel/end 记录 | 事故后无法定位具体阻塞调用 |

## 3. 设计原则

### 3.1 Runtime 唯一拥有 deadline 与终态

Runtime 建立唯一 Tool execution supervisor。Main、Sub、审批后续执行、Agent Tool 与 MCP Tool 都必须经过该 supervisor；Tools 只声明能力并执行，不自行决定 Runtime 终态。

Supervisor 输入至少包含：

- session/run/step/call identity；
- Tool descriptor 的 timeout 与 cancellation declaration；
- Run 冻结配置和调用方更窄的 deadline；
- 用户取消 signal；
- ToolExecutionPort invocation。

有效 deadline 取所有适用 deadline 中最早者。Bash 的用户参数可以缩短该期限，不能突破 Run/Runtime 上限。

### 3.2 硬超时是“前台按时收敛 + 底层状态不撒谎”

硬超时要求前台在 deadline 到达后的有界调度容差内产生 typed terminal outcome。它不等同于声称所有底层工作已被杀死：

- **Cooperative**：Tool 必须周期性检查 cancellation/deadline，停止底层工作并在 grace period 内确认清理完成；确认后记录 `Cancelled` 或 `TimedOut`。
- **NonCooperative**：工作必须隔离，deadline 到达后前台立即收敛；若不能确认底层停止，记录 `CancellationUnconfirmed`、possible side effects 与 unfinished call identity，并阻止危险同名重入。

不得把 drop future 当作取消确认，也不得把 timeout 编码成普通 Internal failure。

### 3.3 不允许阻塞 core async worker

逐一审计内置、MCP 和 Agent Tool：

- 文件遍历、同步 SDK、CPU 重任务必须改为分段可取消算法，或放入隔离 worker/process；
- `spawn_blocking` 只解决 executor 饥饿，不自动提供底层强制终止；若任务超时后仍可能继续，应按 non-cooperative 路径记录；
- Bash 必须管理进程组，timeout/cancel 后发送终止信号、等待 reap，并区分确认与未确认；
- MCP/远端 Agent 必须依据协议是否返回取消确认映射终态，断开本地 future 不代表远端停止。

### 3.4 Tool call lifecycle 必须持久化

Tool call 在 TUI 发布 Running 之前，Runtime 必须先让 Context durable 地接受 pending receipt。后续状态通过幂等 compare-and-advance 更新：

`Pending → Running → Success | Failure | Denied | Cancelled | TimedOut | CancellationUnconfirmed`

同一 call identity 的重复提交必须幂等；terminal 状态不得倒退。Step finalized 仍是消息历史的提交边界，但不能再是 Tool call 首次落盘点。

取消或崩溃恢复时：

- 保留所有已接受的 call identity、tool name、index 和真实已知状态；
- 未确认终止的调用保留 unfinished IDs 与 possible side effects；
- 为 provider 修复消息完整性时可以合成 tool result，但必须显式表达 timeout/cancellation-unconfirmed，不能伪造成功或普通失败；
- 兼容旧 Session Envelope；未知 future schema 继续 fail-closed。

## 4. Published Language 与边界

优先扩展已有 Published Language，避免新建平行状态模型：

- `CancellationDeclaration`：继续表达 Tool 的协作取消能力；
- `ToolOutcomeKind`：新增独立 `TimedOut`，保留 `CancellationUnconfirmed`；
- `StepReceipt`：承载 summary、artifact refs、possible side effects、unfinished call IDs；
- 增加 pending/running receipt mutation OHS，所有权归 Context Management；Storage 只提供 AtomicBlob 机制；
- Runtime supervisor 负责把执行事实映射为 Context mutation，不允许 Tools 或 TUI 直接写 Session。

不新增 Runtime 第二份持久化 Session backing，不恢复 loop-exit `save_chain`。

## 5. 执行入口收敛清单

实现前必须盘点并逐项证明：

1. Main 普通内置 Tool；
2. Main 并发 Tool batch；
3. Sub Run 普通 Tool；
4. Agent Tool 及派生 Run；
5. MCP Tool/资源调用；
6. AskUser 之外的审批后续 Tool 执行；
7. Bash 参数 timeout 与进程组清理；
8. Hook 若包围 Tool 的执行，不能吞掉 deadline/cancel；
9. auto-save/Run terminate/Step cancel 的收敛路径。

目标是所有生产入口只有一个 supervisor；旧 timeout 包装器和旁路必须删除或变成该 supervisor 的窄适配器，并由静态守卫或契约测试防回流。

## 6. 日志与诊断

逐调用日志使用 owner 的 `crate::LOG_TARGET`，不新增独立日志文件。建议事件：

- dispatch accepted；
- execution started；
- deadline reached；
- cancellation requested；
- cancellation confirmed/unconfirmed；
- terminal receipt committed。

字段至少可关联 session/run/step/call/tool、effective timeout、elapsed、cancellation declaration、terminal outcome。输入只记录安全截断摘要；NEVER 记录完整敏感 Tool input。正常取消用 debug，未确认副作用和持久化失败用 warn/error。

## 7. 测试矩阵

所有核心逻辑严格 TDD，跨层链路每层都必须有相邻测试。

### L1

- 可取消目录遍历在 deadline/cancel 检查点停止；
- cooperative/non-cooperative 终态映射；
- Bash 终止进程组与 reap；
- receipt 状态单调性、重复更新幂等、terminal 不倒退。

### L2

- Runtime supervisor 使用 paused Tokio time 或可控 Clock 验证 120 秒边界；不使用短 sleep 证明；
- 同步阻塞 fixture 被隔离，不阻塞 timer、取消处理和其他 Tool；
- Context pending/running/terminal mutation 原子推进；
- `StepReceipt` 所有字段完整编码、解码和恢复。

### L3

- Catalog descriptor → Runtime supervisor → ToolExecutionPort 的 deadline/cancellation 契约；
- Runtime → Context receipt mutation 的 identity 和字段完整性；
- Context → Storage AtomicBlob 的 reopen/previous 恢复兼容。

### L4

- 并发 Glob + Read 中一个调用阻塞，deadline/用户取消后其他调用和 Step 可收敛；
- Tool running 时模拟 auto-save 强制 abort，再 reopen：`**/archify.mjs` call 仍可见且状态真实；
- provider 消息完整性修复不会把 unfinished call 伪装成成功；
- Main、Sub、审批续执行、Agent、MCP stub 使用同一 taxonomy。

### L5

真实 TUI smoke：慢 Glob、长 Bash、MCP stub 各执行一次，验证可取消、UI 恢复、session 可恢复、日志可关联。120 秒默认值可在测试配置中缩短，但生产语义必须等价。

## 8. 实施阶段

1. **失败证据与入口盘点**：固定 `**/archify.mjs` 丢失场景，列出所有 timeout/cancel 旁路。
2. **Published Language**：明确 deadline 和 terminal taxonomy，先写 domain/contract 测试。
3. **Runtime supervisor**：收敛 Main/Sub/Agent/MCP/审批路径，建立有界前台收敛。
4. **Tool 隔离与协作取消**：优先修复 Glob/文件类，再覆盖 Bash、MCP 和其余 Tool adapter。
5. **Durable receipts**：Context 接受 pending/running/terminal mutation，Runtime 接通；Storage 保持机制层。
6. **恢复投影**：合成真实、provider-safe 的 unfinished tool result，并保证幂等。
7. **日志与守卫**：增加生命周期诊断，阻止新执行旁路。
8. **跨层验证与退役**：删除旧 timeout wrappers、空 receipts 和死代码，执行完整门禁。

该工作跨 Runtime、Tools、Context/Storage，若实现评估确认需要多个独立 PR，必须先向用户申请按 GitHub 原生 sub-issues 拆分；不得自行拆 Issue。

## 9. 验收标准

- 任意 Tool 到达 effective deadline 后，前台在有界调度容差内返回 typed timeout；
- core async worker 不再执行不可让出的长同步工作；
- 所有生产 Tool 入口经过唯一 Runtime supervisor；
- cooperative 只有在底层清理确认后才记录 cancelled/timed out；否则记录 unconfirmed；
- 每个已展示/已派发 Tool call 都能在重启后恢复；
- 不再出现“用户在制止一个 Session 中完全不存在的调用”；
- 旧 Session 可读、未来 schema fail-closed、重复恢复幂等；
- 日志足以重建调用生命周期且不泄露敏感输入；
- 相关 crate 测试、CLI 场景、clippy、workspace test 与架构守卫通过。

## 10. 最小补丁与根因方案取舍

### 最小补丁（不推荐作为 Issue 完成条件）

仅将 Glob 放入 `spawn_blocking`，外层保留 timeout，并在超时后返回错误。优点是改动小、能避免 Glob 饿死 Tokio worker；缺点是后台扫描可能继续、其他 Tool 仍可复发、调用仍可能在 abort 时从 Session 消失。

### 根因方案（本 Issue 的完成条件）

统一 Runtime supervisor、执行隔离/协作取消、typed terminal receipt、pending durable checkpoint 与恢复投影。成本较高且跨层，但能同时解决所有 Tool 的硬超时、取消真实性、会话可恢复性和诊断缺口。
