# Audit Context 当前设计

## 1. 责任

Audit 只负责记录和查询模型调用 Usage 事实。唯一持久事实是版本化、按 Session 分区的 JSONL；worker queue、运行状态与关闭状态都不是审计事实。

Audit 不保存 prompt、response、thinking、Tool/Hook 原文、Cost、Price 或 PricingSnapshot。

## 2. Usage Published Language

`UsageRecord` 使用 SDK 统一关联 ID：

- `SessionId`
- `RunId`
- `RunStepId`
- `ModelInvocationId`

并记录 provider、model、时间及 input/output/cache/reasoning token。`UsageEnvelopeV1` 提供稳定 schema version；未知版本或损坏行由查询层跳过并报告 warning。

Runtime 拥有非阻塞 `UsageSink` trait；Audit 发布 `UsageSender` 与 `UsageEmitOutcome`：

- `Accepted`：record 已进入 bounded queue，不代表已经落盘；
- `Dropped(QueueFull)`：queue 已满；
- `Dropped(WorkerUnavailable)`：worker channel 已关闭或 shutdown 已开始。

Audit 不累计以上结果。

## 3. 无状态 worker

Composition 为一个 frontend/runtime 生命周期装配一套 bounded sender 和 `UsageWorker`。Main/Sub 共用 sender；worker 不绑定或保存当前 Session。

worker 顺序处理每条 `UsageRecord`：

```text
receive record
  → encode UsageEnvelopeV1 + newline
  → derive stream from record.session_id
  → append
  → flush
  → receive next
```

运行中 Session 切换无需重建 worker。新旧 records 根据各自 canonical `session_id` 自然写入不同 JSONL 分区。

append 失败时跳过该 record 的 flush并继续下一条；flush 失败时同样继续。失败只写入 `aemeath:diagnostic:audit` warning，不保存累计指标，也不改变 Run 结果。

## 4. 非阻塞 sender

`UsageSender::try_record` 只在短临界区读取 bounded channel slot并调用 `try_send`，绝不 await、序列化或执行 I/O。

shutdown 取走 channel slot 后，所有既有 sender clone 立即返回 `WorkerUnavailable`；已经 Accepted 的 records 由 receiver 尽力 drain。

默认 queue capacity 为 1024，shutdown timeout 为 5 秒。配置在 Composition bootstrap 时按值捕获；运行期配置更新不重启 worker。

## 5. 消费式关闭

`UsageWorker::shutdown(self)` 消费唯一 owner：

1. 关闭 sender slot；
2. 等待 receiver drain 已接受 records；
3. 在 timeout 内结束则返回 `()`；
4. timeout 时 abort worker、等待 abort 完成并记录 warning；
5. 不发布 `Drained`/`TimedOut` 终态，不计算 unconfirmed。

owner 未显式 shutdown 便被 Drop，或 shutdown Future 被取消时，Drop 关闭 sender并 abort worker。异常路径允许尚未写入 JSONL 的 queue 内容丢失，符合 Usage 审计不阻塞 Runtime 的边界。

没有 worker metrics、lifecycle enum、completion cache、shutdown coordinator 或 per-Session worker registry。

## 6. JSONL 存储

默认布局：

```text
~/.agents/audit/
└── usage/
    └── {session_partition}.jsonl
```

每条成功编码的 envelope 形成一条终结 JSONL。文件属于 Audit，不属于 Session storage：

- Session 保存/resume 不读写 Usage；
- Session 删除不级联删除 Usage；
- v0.1.0 不提供自动 retention；
- 既有 `cost_history.json` 不读、不导入、不覆盖、不删除。

`UsageAppendStorePort` 由 Audit 拥有；File adapter 只复用 Storage 发布的路径安全 primitive，并负责 no-follow append、flush、read/list 与 per-stream 互斥。

## 7. 全局 UsageQuery

`UsageQueryPort` 支持：

- 指定 Session 查询一个分区；
- 不指定 Session 时查询全部分区；
- 按 Run/RunStep/Invocation/provider/model/半开时间范围过滤；
- 跨分区 opaque cursor 分页；
- 从原始 JSONL 即时计算全局或过滤后的 token summary。

全局 summary 不落盘，也不修改 worker 状态。CLI/TUI 不直接解析 JSONL。

## 8. Composition 与依赖方向

Composition：

1. 创建 Audit File AppendLog adapter；
2. 创建 bounded sender 与唯一 `UsageWorker` owner；
3. 用 `AuditUsageSink` bridge 实现 Runtime-owned `UsageSink`；
4. 将同一个 sink 注入 Main/Sub 共用的 `RuntimeContextFactory`；
5. frontend 结束后消费 Audit owner执行 drain。

依赖方向：

```text
Runtime → Audit PL + Runtime-owned UsageSink
Composition bridge → Runtime UsageSink + Audit sender/worker
Audit worker → UsageAppendStorePort
Audit File adapter → Storage path-safety PL
CLI/TUI → UsageQueryPort → Audit
```

## 9. 不变量

- **MUST** Runtime 写 Usage 时非阻塞。
- **MUST** Usage 只含 metadata，不含原文和 Cost。
- **MUST** 每条 record 根据自身 Session ID 分区。
- **MUST** 每条成功 append 后请求 flush。
- **MUST** JSONL 是唯一持久审计事实。
- **MUST** 全局 UsageQuery 从 JSONL 即时计算，不保存跨 Session 累计状态。
- **MUST NOT** Audit 失败影响 Run 状态。
- **MUST NOT** worker 保存当前 Session、metrics 或 shutdown outcome。
- **MUST NOT** CLI/TUI 直接解析 JSONL。

## 10. 物理目录

```text
src/
├── lib.rs
├── domain.rs
├── domain/
│   └── usage.rs
├── ports.rs
├── application.rs
├── application/
│   ├── ingest.rs
│   └── query.rs
├── adapters.rs
└── adapters/
    ├── append.rs
    └── query.rs
```

Audit domain/application/worker 不直接访问 filesystem；具体文件 I/O 终止在 Audit-owned adapter。

## 11. 相关文档

- Usage 存储与测试矩阵：[01-usage-storage.md](01-usage-storage.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Migration：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-08-24 | 将 Usage worker 收窄为无状态 JSONL 写入机制：删除运行指标与共享 shutdown 终态，采用消费式 drain；保持按 record Session 分区和全局查询 | Usage JSONL 单一事实设计 |
| 2026-08-10 | 完成 Usage-only MVP 的分层测试验收 | 测试治理记录 |
| 2026-08-09 | 完成 Runtime UsageSink、Composition bridge 与旧 Cost surface 退役 | Usage-only MVP |
| 2026-07-21 | 完成全字段过滤、分页、损坏行隔离与 token summary | Usage Query |
| 2026-07-17 | 冻结 Usage V1 schema 与 AppendLog 边界 | Usage Contracts |
