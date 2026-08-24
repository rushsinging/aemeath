# Audit Session JSONL 单一事实设计

## 1. 背景

Audit Usage 当前由 bounded worker 异步写入 JSONL。worker 额外维护 accepted、completed、dropped、write-failed、drain-abandoned 等累计指标，并通过可重复调用的 shutdown handle 发布 `Drained` 或 `TimedOut` 终态。

这些运行期指标和共享终态不是 Usage 审计的领域事实，却引入了多组 Mutex、生命周期状态与并发关闭协调问题。尤其是多个调用者并发执行 shutdown 时，一个调用者可能取走 worker join handle，另一个调用者则在最终结果尚未产生时错误返回 `Drained`。

本设计收窄 Audit Usage：**唯一持久事实是按 Session 分区的 Usage JSONL**。worker 只是非阻塞写入机制，不再暴露或保存运行指标与 shutdown 终态。

## 2. 目标

1. 每个 Session 拥有独立的 Usage sender、bounded queue、worker 和 JSONL 分区。
2. shutdown 采用消费所有权的一次性 API，从类型层消除并发重复关闭。
3. 删除 worker metrics、shutdown outcome、completion cache 和 coordinator 概念。
4. 显式 shutdown 尽力在配置超时内 drain；超时只记录诊断日志并停止 worker，不产生领域终态。
5. `UsageQuery` 保持全局只读能力，可以查询一个或全部 Session，并即时汇总。
6. 不保存跨 Session 的累计指标、summary、worker 状态或 shutdown 状态。

## 3. 非目标

1. 不改变 `UsageRecord`、V1 envelope 和现有 JSONL schema。
2. 不迁移现有 `audit/usage/*.jsonl` 布局。
3. 不删除全局 `UsageQuery`、跨分区分页或全局即时 summary。
4. 不建设 workspace/global Usage 汇总文件或可变 read model。
5. 不承诺 runtime 被强制终止、进程崩溃或强杀时完成 drain。
6. 不扩展 worker panic、JoinError 等内部失败为 Published Language 终态。

## 4. 领域边界

### 4.1 Session 写模型

每个 `SessionAudit` 绑定一个确定的 `SessionId`，并独立拥有：

- `UsageSink`；
- bounded sender；
- worker task；
- shutdown timeout；
- 该 Session 对应的 append stream。

不同 Session 可以共享无状态的底层 append store 能力，但不得共享 sender、queue、worker 或关闭控制。

worker 的目标 stream 在装配时由绑定的 `SessionId` 确定，而不是在消费每条 record 时自由选择。record 的 `session_id` 必须与绑定 Session 一致；不一致的 record 不得写入任一 JSONL，并记录诊断 warning。发送端返回结构化 drop reason，使 Runtime 的非阻塞结果保持可观察，但不累计该事件。

### 4.2 全局读模型

`UsageQueryService` 继续读取所有 Session JSONL：

- 指定 `session_id` 时只读取对应分区；
- 未指定 `session_id` 时枚举全部分区；
- 跨分区 cursor 与分页保持不变；
- `summarize()` 在查询时读取原始 records 并即时计算；
- query 和 summary 不回写全局累计文件，也不修改任何 Session worker 状态。

因此，全局 Usage 是可重复计算的查询结果，不是跨 Session 保存的可变聚合。

## 5. 写入模型

### 5.1 保留的生产类型

- `UsageWorkerConfig`：仅保留 queue capacity 与 shutdown timeout。
- `UsageSender`：执行非阻塞 `try_record`。
- `UsageWorker`（现有 handle 可按职责重命名）：唯一拥有 worker join handle，并提供消费式 drain。
- `UsageEmitOutcome` / `UsageDropReason`：表达单次 emit 是否进入队列，不承担累计统计。

### 5.2 删除的生产类型与状态

- `UsagePipelineMetricsSnapshot`；
- accepted/completed/dropped/write-failed/drain-abandoned counters；
- `UsageSender::metrics()`；
- `UsageShutdownOutcome`；
- lifecycle enum；
- completion Mutex；
- 可重复调用的 `shutdown(&self)`；
- single-flight coordinator 或共享 shutdown future。

### 5.3 sender 关闭

sender 只需要共享一个可关闭的 channel slot。正常运行时，`try_record` 克隆 channel sender 并执行 `try_send`：

- 成功返回 `Accepted`；
- queue 满返回 `Dropped(QueueFull)`；
- channel 已关闭或 shutdown 已开始返回 `Dropped(WorkerUnavailable)`；
- record Session 与绑定 Session 不一致时返回专用的 Session mismatch drop reason，并记录诊断 warning。

不为以上结果维护累计计数。

## 6. 消费式 shutdown

### 6.1 API

worker 关闭方法消费其唯一所有者：

```text
shutdown(self) -> Future<Output = ()>
```

`SessionAudit::shutdown(self)` 同样消费 `SessionAudit`。Rust 所有权保证同一实例不能被两个调用者并发或重复关闭，因此不需要 completion cache、状态协调器或共享 outcome。

### 6.2 正常路径

1. 消费 `SessionAudit`。
2. 关闭当前 Session 的 sender slot，使已有 sink clone 立即拒绝新记录。
3. worker 继续处理 channel 中已接受的 records。
4. 每条 record append 并 flush 到当前 Session JSONL。
5. worker 正常结束后 shutdown 返回 `()`。

append 或 flush 失败只记录诊断 warning，继续处理后续 records；不保存 write-failed counter。

### 6.3 timeout

若 worker 未在配置时限内结束：

1. abort worker task；
2. 等待 abort 完成，避免遗留 detached worker；
3. 记录一次包含 Session 标识的诊断 warning；
4. 返回 `()`。

不计算 `unconfirmed`，不增加 abandoned counter，也不发布 `TimedOut` 领域结果。CLI 的 frontend 原始成功或失败结果不受影响。

### 6.4 Drop 与取消

显式 shutdown 是正常释放路径。worker owner 实现 Drop 防线：

- owner 未显式 shutdown 便被丢弃时，立即关闭 sender slot并 abort worker；
- `shutdown(self)` Future 被取消时，owner 随 Future drop，同样 abort worker；
- Drop 不执行异步 drain，也不启动后台 coordinator；
- 异常路径允许丢失尚未写入 JSONL 的队列内容，符合 Usage 审计不阻塞 Runtime 的边界。

为避免正常 shutdown 返回时 Drop 重复 abort，方法在完成或 timeout 后先取走并处理 join handle，再让空 owner drop。

## 7. Composition 与 CLI

### 7.1 Composition

`wire_session_audit` 必须接收当前 `SessionId`，并以此构造 Session-bound sender、worker 与 stream。

Main/Sub 在同一个 Session 运行上下文内可以共享当前 Session 的 `UsageSink`；不同 Session 每次独立装配。底层 JSONL store 可以共享存储根，但 Session runtime state 不共享。

`SessionAudit` 只暴露：

- 克隆当前 Session `UsageSink`；
- 消费自身执行 shutdown。

不暴露 metrics 或 shutdown outcome。

### 7.2 CLI

CLI frontend helper 接收一个消费式 drain future，职责仅为：

1. 运行 frontend 并保存原始结果；
2. 消费当前 Session 的 Audit owner并等待 drain；
3. 返回原始 frontend 结果。

CLI 不解释 Audit 终态，也不需要 typed outcome capability。成功/失败测试只需证明 drain 被执行且不覆盖 frontend 原始结果。

bootstrap 需要把 `Option<SessionAudit>` 的所有权移动给 frontend 收尾路径；不得通过共享引用留下可重复 shutdown API。

## 8. 数据与错误处理

### 8.1 唯一事实

只有编码成功并交给 append store 的 `UsageEnvelopeV1` JSONL 行才可能参与后续 UsageQuery；flush 是持久化尽力保证，但 flush 失败不允许用内存计数伪造或删除已经 append 的事实。内存计数、queue 状态和 shutdown 状态都不是审计事实，不落盘、不恢复、不跨 Session 汇总。

### 8.2 非阻塞保证

Runtime 调用 `try_record` 不等待磁盘：

- queue 满时立即 drop；
- worker 不可用时立即 drop；
- Session 不匹配时立即 drop；
- emit outcome 不改写模型调用结果。

### 8.3 诊断日志

以下异常保留诊断日志，但不形成持久指标：

- Session mismatch；
- append 失败；
- flush 失败；
- shutdown timeout；
- worker owner 未经显式 shutdown 被丢弃。

日志遵循 Audit 已注册 target 和日志 schema，不记录 prompt、response 或其他敏感原文。

## 9. 测试策略

严格按 TDD 执行，先验证旧 API/行为不能满足目标，再修改生产代码。

### 9.1 Audit application

1. worker 绑定 Session，匹配 record 写入对应 stream。
2. Session mismatch 立即 drop，且不写入任一 stream。
3. queue 满仍立即返回 `QueueFull`。
4. append 失败后继续处理下一条 record。
5. flush 失败后继续处理下一条 record。
6. 消费式 shutdown drain 已接受 records 并返回 `()`。
7. shutdown timeout abort worker，不留下 detached task。
8. owner 未显式 shutdown时 Drop abort worker。
9. 删除所有 metrics 和重复 shutdown 契约测试。

### 9.2 Audit public contract

1. crate-root worker API 只能被消费一次。
2. shutdown 后当前 Session 的 JSONL 包含已成功处理 records。
3. late sender clone 返回 `WorkerUnavailable`。
4. `UsageQuery` 仍能全局读取多个 Session 分区并即时汇总。

消费一次主要由 Rust 所有权和 compile check 保证，不新增生产 test-only API。

### 9.3 Composition

1. Session ID 从装配入口传到 worker 分区。
2. 两个 Session 分别写入不同 JSONL 分区。
3. 关闭 Session A 不影响 Session B 写入。
4. 全局 query 能读取 A、B records。
5. `SessionAudit::shutdown(self)` 消费 owner且完成 drain。

### 9.4 CLI

1. frontend 成功时执行 drain 并保留成功。
2. frontend 失败时执行 drain 并保留原始错误。
3. Audit 不存在时保留原始结果。
4. 不再测试 `Drained` / `TimedOut`，因为该 Published Language 被删除。

### 9.5 门禁

- Audit、Runtime、Composition、CLI 相邻边界定向测试；
- production-only check/clippy；
- `cargo test --workspace`；
- `cargo clippy --workspace --all-targets -- -D warnings`；
- `cargo fmt --all --check`；
- 完整架构守卫；
- coverage gate；
- 废弃 API、测试和设计文档引用清理。

## 10. 迁移与兼容

这是生产 Rust API 的破坏性收窄，但不改变磁盘格式：

- 调用方从借用式重复 shutdown 改为所有权消费；
- 删除 worker metrics 和 shutdown outcome 的导出与 façade 白名单；
- 既有 JSONL 继续由全局 UsageQuery 读取；
- 不创建迁移文件，不修改 schema version；
- 设计文档的测试矩阵必须从“指标、精确 timeout outcome、幂等 shutdown”改为“Session-bound JSONL、消费式 drain、异常 Drop abort”。

## 11. 验收标准

1. Audit 生产代码中不存在 worker metrics、shutdown outcome、completion cache 或 coordinator。
2. 同一 Session worker 的 shutdown 在类型层只能消费执行一次。
3. 每个 Session 只写自己的 JSONL 分区，错 Session record 不污染存储。
4. 显式 shutdown 在时限内 drain，超时或取消时 abort，不遗留 detached worker。
5. Runtime emit 始终非阻塞且不被 Audit drop 改写。
6. `UsageQuery` 可查询一个或全部 Session，并即时计算全局 summary。
7. 不存在跨 Session 持久累计指标或 summary 文件。
8. Audit、Runtime、Composition、CLI 每个相邻边界都有对应测试。
9. 全部验证门禁通过，设计文档、Issue 验收项与生产行为一致。
