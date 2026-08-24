# Audit Usage JSONL 单一事实设计

## 1. 背景

Audit Usage 当前通过 bounded worker 异步写入 JSONL。worker 还维护 accepted、completed、dropped、write-failed、drain-abandoned 等累计指标，并通过可重复调用的 shutdown handle 发布 `Drained` 或 `TimedOut` 终态。

这些运行期指标和共享终态不是 Usage 审计事实，却引入了多组 Mutex、生命周期状态和并发关闭竞态。多个调用者并发执行 shutdown 时，一个调用者可能取走 worker join handle，另一个调用者则在最终结果尚未产生时错误返回 `Drained`。

本设计将 Audit Usage 收窄为：**唯一持久事实是按 `UsageRecord.session_id` 分区的 Usage JSONL**。worker 只是 frontend/runtime 生命周期内的非阻塞写入机制，不保存 Session 状态、运行指标或 shutdown 终态。

## 2. 目标

1. 每条 Usage record 只追加到自身 `session_id` 对应的 JSONL 分区。
2. 删除 worker metrics、shutdown outcome、completion cache、lifecycle enum 和 coordinator 概念。
3. shutdown 采用消费所有权的一次性 API，从类型层消除并发重复关闭。
4. 显式 shutdown 尽力在配置超时内 drain；超时只记录诊断日志并停止 worker。
5. `UsageQuery` 保持全局只读能力，可以查询一个或全部 Session，并即时汇总。
6. 不保存跨 Session 的累计指标、summary、worker 状态或 shutdown 状态。
7. 保持运行中 Session 切换：同一 worker 可依次处理不同 Session 的 records，但自身不累计或绑定任何 Session。

## 3. 非目标

1. 不改变 `UsageRecord`、V1 envelope 和现有 JSONL schema。
2. 不迁移现有 `audit/usage/*.jsonl` 布局。
3. 不删除全局 `UsageQuery`、跨分区分页或全局即时 summary。
4. 不建设 workspace/global Usage 汇总文件或可变 read model。
5. 不创建 per-Session worker registry、metrics registry 或 shutdown coordinator。
6. 不承诺 runtime 被强制终止、进程崩溃或强杀时完成 drain。
7. 不扩展 worker panic、JoinError 等内部失败为 Published Language 终态。

## 4. 边界与数据流

### 4.1 写入

Composition 为一个 frontend/runtime 生命周期创建一套：

- `UsageSender`；
- bounded queue；
- Usage worker；
- shutdown timeout；
- Audit append store。

Main/Sub 共用该 sender。每条 `UsageRecord` 自带 canonical `session_id`，worker 对每条 record 计算 `AppendLogStream::for_session(record.session_id)`，因此运行中发生 Session 切换时，新旧 Session records 会自然进入不同 JSONL 文件。

worker 不保存“当前 Session”，也不维护跨 Session 或 per-Session 的内存累计状态。Session 隔离由 JSONL 分区保证：一条 record 只能写入由自身 `session_id` 派生的 stream。

### 4.2 查询

`UsageQueryService` 继续作为全局只读能力：

- 指定 `session_id` 时只读取对应分区；
- 未指定 `session_id` 时枚举全部分区；
- 跨分区 cursor 与分页保持不变；
- `summarize()` 在查询时读取原始 records 并即时计算；
- query 和 summary 不回写全局累计文件，也不修改 worker 状态。

全局 Usage 是从 JSONL 重算的查询结果，不是跨 Session 保存的可变聚合。

## 5. 写入模型

### 5.1 保留类型

- `UsageWorkerConfig`：queue capacity 与 shutdown timeout。
- `UsageSender`：执行非阻塞 `try_record`。
- `UsageWorker`：唯一拥有 worker join handle并提供消费式 shutdown。
- `UsageEmitOutcome` / `UsageDropReason`：表达单次 emit 是否进入队列。

`UsageEmitOutcome` 是 Runtime hot path 的即时返回值，不是统计值，不落盘。

### 5.2 删除类型与状态

- `UsagePipelineMetricsSnapshot`；
- accepted/completed/dropped/write-failed/drain-abandoned counters；
- `UsageSender::metrics()`；
- `UsageShutdownOutcome`；
- lifecycle enum；
- completion Mutex；
- 可重复调用的 `shutdown(&self)`；
- single-flight coordinator、共享 shutdown future 或 per-Session worker registry。

### 5.3 sender

sender 只共享一个可关闭的 bounded channel slot。`try_record` 的结果为：

- enqueue 成功：`Accepted`；
- queue 满：`Dropped(QueueFull)`；
- shutdown 已开始或 worker channel 已关闭：`Dropped(WorkerUnavailable)`。

不为以上结果维护累计计数。`Accepted` 只表示进入队列，不表示已经写入或 flush。

## 6. worker 与消费式 shutdown

### 6.1 worker

worker 顺序处理每条 record：

1. 编码 `UsageEnvelopeV1` 与终结换行；
2. 从该 record 的 `session_id` 派生 stream；
3. append 到该 Session JSONL；
4. flush 该 stream；
5. 继续处理下一条 record。

append 或 flush 失败只记录诊断 warning，且不阻塞 Runtime、不回滚模型调用。append 失败时跳过该 record 的 flush；flush 失败后继续下一条 record。

### 6.2 API

worker owner 通过消费所有权关闭：

```text
shutdown(self) -> Future<Output = ()>
```

Composition 的 Audit owner 同样以 `shutdown(self)` 消费自身。Rust 所有权保证同一实例不能被两个调用者并发或重复关闭，因此不需要共享完成状态。

### 6.3 正常 drain

1. 消费 Audit owner。
2. 关闭共享 sender slot，使已有 sink clone 立即返回 `WorkerUnavailable`。
3. receiver drain 已进入 queue 的 records。
4. worker 正常结束后 shutdown 返回 `()`。

### 6.4 timeout

worker 未在配置时限内结束时：

1. abort worker task；
2. await abort 完成，避免 detached worker；
3. 记录一次诊断 warning；
4. 返回 `()`。

不计算 `unconfirmed`，不增加 abandoned counter，不发布 `TimedOut` 结果。frontend 原始成功或失败结果保持权威。

### 6.5 Drop 与取消

worker owner 实现 Drop 防线：

- 未显式 shutdown 便被丢弃时，立即关闭 sender slot并 abort worker；
- `shutdown(self)` Future 被取消时，owner 随 Future drop并 abort worker；
- Drop 不异步 drain，也不启动后台任务；
- 异常路径允许丢失尚未写入 JSONL 的队列内容，符合非阻塞尽力审计边界。

实现时 join handle 必须在 shutdown await 期间继续保存在 owner 字段中。若先移动到局部变量，Future 取消时局部 `JoinHandle` drop 会 detach，破坏“不遗留 worker”要求。正常完成或 timeout 后再从 owner 中取走已处理的 handle，避免 Drop 重复 abort。

## 7. Composition 与 CLI

### 7.1 Composition

Composition 创建 Audit sender/worker，将同一个 `Arc<dyn UsageSink>` 注入当前 frontend/runtime 的 `RuntimeContextFactory`；Main/Sub 共享该 sink。worker 不绑定 Session，Session 分区完全由 record 的 canonical `session_id` 决定。

Audit owner 只暴露：

- 克隆 `UsageSink`；
- 消费自身执行 shutdown。

不暴露 metrics 或 shutdown outcome。

### 7.2 CLI

CLI frontend helper 保持 Future seam：

1. 运行 frontend 并保存原始结果；
2. 消费 Audit owner并等待 drain future；
3. 返回原始 frontend 结果。

bootstrap 必须把 `Option<Audit owner>` 的所有权移动给 quiet/TUI 收尾路径，不再通过共享引用调用可重复 shutdown。

## 8. 唯一事实与诊断

只有成功编码并交给 append store 的 `UsageEnvelopeV1` JSONL 行才可能参与后续 UsageQuery；flush 是持久化尽力保证。flush 失败不允许用内存统计伪造或删除已经 append 的事实。

以下事件可以写入 `aemeath:diagnostic:audit`，但不形成持久指标：

- append 失败；
- flush 失败；
- shutdown timeout；
- Audit owner 未经显式 shutdown 被丢弃。

日志使用 Audit crate root 的已注册 target，不记录 prompt、response 或其他敏感原文。queue full 和 worker unavailable 已通过单次 `UsageEmitOutcome` 返回，不额外建立累计日志节流状态。

## 9. 测试策略

严格按 TDD 执行。

### 9.1 Audit application

1. queue 满立即返回 `QueueFull`。
2. records 根据各自 Session 写入不同 stream，顺序 append/flush。
3. append 失败后继续处理下一条 record。
4. flush 失败后继续处理下一条 record。
5. 消费式 shutdown drain 已接受 records并返回 `()`。
6. shutdown 开始后 sender clone 返回 `WorkerUnavailable`。
7. shutdown timeout abort worker，不留下 detached task。
8. owner 未显式 shutdown时 Drop abort worker。
9. 删除 metrics 与重复 shutdown 契约测试。

### 9.2 Audit public contract

1. 公开 worker API 使用消费式 shutdown。
2. shutdown 后 JSONL/recording store 包含已成功处理 records。
3. late sender clone 返回 `WorkerUnavailable`。
4. 全局 UsageQuery 仍读取多个 Session 分区并即时汇总。

### 9.3 Runtime 与 Composition

1. Runtime 对 `QueueFull` / `WorkerUnavailable` 的 drop 不改写 invocation 结果。
2. Main/Sub records 保留各自 canonical Session ID。
3. 两个 Session records 落入不同 JSONL 分区。
4. 全局 query 能读取并汇总两个 Session。
5. Composition Audit owner 通过 `shutdown(self)` 完成 drain。

### 9.4 CLI

1. frontend 成功时执行 drain并保留成功。
2. frontend 失败时执行 drain并保留原始错误。
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
- 废弃 API、测试、Guard 白名单和设计文档引用清理。

## 10. 兼容与验收

生产 Rust API 会收窄，但磁盘格式保持兼容：

- 调用方从借用式重复 shutdown 改为所有权消费；
- 删除 worker metrics 与 shutdown outcome 导出；
- 既有 JSONL 继续由全局 UsageQuery 读取；
- 不创建迁移文件，不修改 schema version。

验收标准：

1. Audit 生产代码不存在 worker metrics、shutdown outcome、completion cache、coordinator 或 per-Session worker registry。
2. shutdown 在类型层只能消费执行一次。
3. 每条 record 只写入自身 Session JSONL，worker 不保存当前 Session。
4. 显式 shutdown 在时限内 drain，超时或取消时 abort，不遗留 detached worker。
5. Runtime emit 始终非阻塞且 Audit drop 不改写 invocation 结果。
6. `UsageQuery` 可查询一个或全部 Session，并即时计算全局 summary。
7. 不存在跨 Session 持久累计指标或 summary 文件。
8. Audit、Runtime、Composition、CLI 相邻边界测试及全部门禁通过。
