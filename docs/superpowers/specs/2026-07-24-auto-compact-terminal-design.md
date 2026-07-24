# Auto-compact Skipped 与 Run Terminal 收口设计

> 对应 Issue：[#1387](https://github.com/rushsinging/aemeath/issues/1387)  
> Milestone：`v0.1.0 — Context Engineering + 架构重构`

## 1. 问题

恢复大型 Session 后，Context 的预检查判定需要自动 compact，但实际 compact
返回 `CompactOutcome::Skipped(ResumeProtection)`。Main/Sub adapter 将这个
typed skip 转成 `LoopEngineError::Adapter`，共享 `run_loop` 因而提前返回错误。

`RunLauncher` 收到错误后只记录日志并返回 `RunLaunchResult::Failed`，没有让
Run 聚合进入 `Failed`，也没有发布 `RunDomainEvent::Failed`。Main caller
同样只记录日志，因此 SDK/TUI 没有收到 terminal event，spinner 持续显示
`Thinking`，而进程实际已经空闲。

## 2. 设计目标

1. 自动 compact 的 `Skipped` 是非致命 no-op，当前 Run 继续调用模型。
2. shared loop 的未处理错误必须由唯一生命周期边界转成 Run 的权威失败终态。
3. Main 与 Sub 使用完全相同的失败收口，不在 TUI、SDK 或 caller 添加旁路事件。
4. 不修改 Session backing，不改变 manual compact 的用户提示语义。

## 3. 方案

### 3.1 Compact adapter

MainRunPort 与 SubAgentRunner 的 `compact` 实现遵循 Context Published Language：

- `Committed`：清空 `last_total_tokens` 和缓存的 `ContextWindow`，下一阶段重建
  window。
- `Skipped`：返回 `Ok(())`，不提交 PreCompact reflection，不清空
  `last_total_tokens`，随后进入当前 step 的模型调用。
- `Err`：继续映射成 `LoopEngineError::Adapter`。

保留 usage 的原因是 compact 并未提交；模型调用成功后会用新的 Provider usage
覆盖它。一次 step 内不会围绕同一个 skip 重试 compact。

### 3.2 RunLauncher terminalization

`RunLauncher` 是 Main/Sub Run 创建、ActiveRun 注册/释放及 terminal 映射的唯一
应用服务。shared `run_loop` 返回未处理错误时，launcher 必须：

1. 通过 `RunLoopPort::claim_terminal` 取得唯一 terminal claim；
2. 调用 Run 聚合的 `fail(error)`；
3. 立即 drain 并通过同一个 port 发布 `RunDomainEvent::Failed`；
4. 保留 `RunLaunchResult::Failed`，供 Main 日志和 Sub terminal fallback 诊断；
5. 无论成功或失败都清理 ActiveRun 注册。

终态始终来自 Run 聚合。Main caller、Sub caller、SDK 和 TUI 只消费投影，不补造
失败事件。

## 4. 错误语义

- `CompactOutcome::Skipped`：业务 no-op，不展示 API error。
- Context compact `Err`：loop adapter error，Run 进入 `Failed`。
- terminal event 发布失败：保留并返回原始 engine error，同时记录 terminalization
  失败；不得把 Run 误报为成功。
- terminal claim 已由取消/终止路径取得：launcher 不竞争补造第二个 terminal。

## 5. 测试

- L2 Main adapter：`Skipped(ResumeProtection)` 返回成功，不触发 PreCompact
  reflection，随后允许模型调用。
- L2 Sub adapter：同一 skipped 语义。
- L2 RunLauncher：adapter error 产生且只产生一个 `RunDomainEvent::Failed`，错误
  文本保真，ActiveRun 被清理。
- L3：复用已有 Runtime domain event → SDK `RunFailed` 投影契约，不修改 TUI
  ACL。
- L0：`cargo fmt --check`、Runtime test、workspace check/clippy、架构守卫。

## 6. 非目标

- 不重构 Context 的 token estimation 或 `ResumeProtection` 命名。
- 不修改 compact 阈值与 Session schema。
- 不新增 TUI timeout/watchdog。
- 不处理与 #1387 无关的 Runtime 重构。

