# Audit（支撑域）

> 层级：02-modules / audit（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> Audit MVP 只记录 Model Usage metadata；不保存原文，不计算 Cost，不阻塞 Runtime。

## 1. 本期范围

v0.1.0 Audit MVP 只包含：

```text
UsageRecord
UsageSink
UsageQueryPort
Usage worker
```

本期不记录 Run lifecycle、Tool、Policy、Hook、Session、Config 或 Workspace 事件。这些继续使用 Domain Event 与 Logging；Future 扩展 AuditEvent 时另行设计 schema 与兼容策略。

Cost/Pricing 在战略层仍是 Audit Future 能力，但本期不查 Price、不计算 Cost、不存 Price、不返回 CostSummary。

## 2. UsageRecord

```rust
struct UsageRecord {
    recorded_at: Timestamp,
    session_id: SessionId,
    run_id: RunId,
    run_step_id: RunStepId,
    model_invocation_id: ModelInvocationId,
    provider: ProviderName,
    model: ModelName,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}
```

- 使用系统统一语言 RunId / RunStepId / ModelInvocationId；
- SessionId 是分区和查询维度，不表示 Usage 属于 Session 聚合；
- 不冗余 parent_run_id，父子关系由 Run 模型解释；
- Provider 负责从供应商响应提取 raw usage，Runtime 在 Model Invocation 完成后构造 UsageRecord。

## 3. 默认内容策略

Audit 只记录 metadata，禁止默认保存：

- 用户 prompt 与完整上下文；
- 模型 response / thinking；
- Tool input/output；
- Hook stdout/stderr；
- 环境变量与密钥；
- Cost、Price 或 PricingSnapshot。

可选原文审计不在本期范围；如未来引入，必须先设计脱敏、加密、访问控制、用户 opt-in 与 retention。

## 4. 非阻塞 UsageSink

> `UsageSink` trait 由 **Runtime 拥有**并定义在 [runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)（`fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome`）；Audit adapter 实现该 trait，但 Audit 本文档 **NEVER** 重复定义签名，只发布该签名引用的结果类型：

```rust
enum UsageEmitOutcome {
    Accepted,
    Dropped(UsageDropReason),
}

enum UsageDropReason {
    QueueFull,
    WorkerUnavailable,
}
```

- Runtime hot path 只做 try_record，永不 await Audit IO；
- Runtime 不重试、不改变 Run 状态；
- queue bounded；Composition 在 bootstrap / active wiring 发布前从已验证 `ConfigSnapshot` 提取 `UsageQueueConfig { capacity }` 并按值注入 sink，Audit worker **NEVER** 持有 `ConfigReader`、`ConfigQuery` 或裸 Config 字段；
- Dropped 由 sink 计数，并通过 Logging 低频聚合 warn；
- enqueue Accepted 只表示进入队列，不表示 worker 已接收或已经落盘；Accepted 到 flush 完成之间存在进程异常导致的静默丢失窗口，这是尽力审计的明确语义。

`WorkerUnavailable` 仅在以下状态返回：Composition 尚未完成 worker 启动、worker 已不可恢复退出且 sender 关闭、或 graceful shutdown 已开始且不再接收新记录。

## 5. Worker 与写入语义

Usage worker 顺序消费 queue：

```text
receive one UsageRecord
  → UsageAppendStorePort.append(partition, record)
  → UsageAppendStorePort.flush(partition)
  → receive next
```

本期选择每条记录 append 后立即 flush。

写入失败：

- `write_failed_total` 增加；
- 输出聚合诊断日志；
- 不回传到已经完成的 Model Invocation；
- 不阻塞或回滚 Run；
- 正常退出时尽力 drain queue。

正常退出采用固定时序：

1. Composition 将 sink 标记为 shutting down，后续 try_record 返回 WorkerUnavailable；
2. 关闭 queue sender；
3. worker drain 已接受记录，并逐条 append + flush；
4. 等待 ConfigSnapshot `usage_shutdown_timeout()` accessor 提供的时限；
5. 超时后放弃剩余记录，增加 `drain_abandoned_total` 并记录聚合 warn。

Audit 是尽力事实记录，不是 durable execution 或 Run checkpoint。

## 6. 存储分区

逻辑路径：

```text
~/.agents/audit/
└── usage/
    └── {session_id}.jsonl
```

- 每条 JSONL 对应一个 UsageRecord；
- 文件属于 Audit，不属于 Session storage；
- Session 保存/resume 不读写 Usage；
- Session 删除不级联删除 Usage；
- v0.1.0 不提供自动 retention 或删除命令；
- Future retention 由 Audit 决定，不由 Context Management 决定。

路径只是默认 File AppendLog Adapter 的映射；Audit domain 不直接拼接或访问该路径。

## 7. UsageAppendStorePort（Audit-owned 出站端口）

> `UsageAppendStorePort` 是 **Audit BC 拥有的出站端口**。整值原子替换协议（Storage 的 `AtomicBlobPort`/`AtomicDatasetPort`）建立在 stage → fsync → rename 之上，拼不出增量 append + 逐行 flush 语义；因此 Audit 的默认 File AppendLog Adapter **MUST** 直接以 file append（open-append 等价物 + write + fsync）detail 实现追加写入，只复用 Storage 发布的路径安全 primitive（`SafePathSegment` 校验、受约束根目录句柄解析），**NEVER** 组合调用 `AtomicBlobPort`/`AtomicDatasetPort` 来模拟 append。端口 trait 定义、调用和语义归属均属 Audit；Storage **不** 发布通用 AppendLogPort OHS。

```rust
struct AppendLogNamespace(String);      // Audit 使用 "usage"
struct AppendLogStream(String);         // 不透明 stream key；Audit adapter 从 SessionId 派生

struct AppendLogReader {
    lines: AsyncLineStream,
}

enum AppendLogError {
    Io,
    InvalidNamespace,
    InvalidStream,
    Closed,
}

trait UsageAppendStorePort: Send + Sync {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError>;
    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError>;
    async fn read(&self, stream: &AppendLogStream) -> Result<AppendLogReader, AppendLogError>;
    async fn list_streams(
        &self,
        namespace: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError>;
}
```

AppendLogStream 是 Audit adapter 自己解析出的不透明 stream key，不暴露绝对路径；Audit adapter 负责 `SessionId → AppendLogStream` 映射以及 `namespace/stream → 物理路径` 的安全解析，解析时复用 Storage 发布的路径安全 primitive，而不是把该映射交给某个 Storage 端口。

Audit adapter（detail 执行机制）负责：

- 路径映射与目录创建（基于 Storage 路径安全 primitive，而非 Storage OHS）；
- append 与 flush 的物理执行；
- 顺序读取与 namespace 下的 stream 枚举；
- 轮转/分段的物理文件切分、rename 等执行机制；
- 文件/IO 层错误隔离。

Audit（决策）负责：

- Usage schema；
- JSONL 编码语义；
- SessionId 分区策略；
- **何时/按何种策略触发轮转**（大小、时间、条数等阈值）——轮转是 Audit 的业务决策，adapter 只执行 Audit 下达的物理切分指令，不得自行决定是否轮转；
- 查询与 token 聚合；
- Future retention 策略。

UsageAppendStorePort 是 Audit-owned 出站端口，adapter 直接以 file append detail 实现，不得命名为 SessionLogPort 或让 Storage 解释 Usage 字段。

## 8. UsageQueryPort

```rust
trait UsageQueryPort: Send + Sync {
    async fn query(&self, query: UsageQuery) -> Result<UsagePage, UsageQueryError>;
    async fn summarize(&self, query: UsageQuery) -> Result<UsageSummary, UsageQueryError>;
}

struct UsageQuery {
    session_id: Option<SessionId>,
    run_id: Option<RunId>,
    run_step_id: Option<RunStepId>,
    model_invocation_id: Option<ModelInvocationId>,
    provider: Option<ProviderName>,
    model: Option<ModelName>,
    recorded_range: Option<TimeRange>,
    pagination: Pagination,
}

struct TimeRange {
    from_inclusive: Option<Timestamp>,
    to_exclusive: Option<Timestamp>,
}

struct Pagination {
    cursor: Option<UsageCursor>,        // opaque: partition + line offset
    limit: NonZeroUsize,
}

struct UsagePage {
    records: Vec<UsageRecord>,
    next_cursor: Option<UsageCursor>,
    warnings: Vec<UsageQueryWarning>,
}

struct UsageSummary {
    record_count: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_tokens: u64,
    cache_read_tokens: u64,
    reasoning_tokens: u64,
}

enum UsageQueryError {
    Storage(AppendLogError),
    InvalidRange,
    InvalidCursor,
}

enum UsageQueryWarning {
    CorruptLine { stream: AppendLogStream, line_number: u64 },
}
```

`UsageCursor` 对调用方不透明，只用于稳定续页；limit 上限由 Audit query config 校验。

UsageSummary 只汇总 token 与记录数，不包含 Cost。查询实现读取 Audit 分区并在 BC 内聚合，CLI/TUI 不直接解析 JSONL。

运行指标可独立查询：

```text
accepted_total
dropped_total { queue_full, worker_unavailable }
write_failed_total
drain_abandoned_total
```

这些指标描述 Audit pipeline，不写回 Usage JSONL。

## 9. Composition 与依赖方向

Composition Root：

1. 创建 Audit 自己的 File AppendLog adapter（`UsageAppendStorePort` 实现，直接以 file append detail 落盘，只复用 Storage 发布的路径安全 primitive）；
2. 创建 bounded queue 和 Usage worker；
3. 向 Runtime 注入 `Arc<dyn UsageSink>`；
4. 向 CLI/TUI/Server 查询用例提供 UsageQueryPort；
5. 负责 shutdown drain/flush。

依赖方向：

```text
Runtime → UsageSink PL
Audit worker → UsageAppendStorePort
Audit File AppendLog adapter → filesystem（direct file append detail）
CLI/TUI → UsageQueryPort → Audit
```

Audit domain/worker 不依赖 Runtime、TUI、Logging 具体实现或直接拼接文件路径；实际文件系统访问被封装在 Audit 自己的 File AppendLog adapter 内，只经由 `UsageAppendStorePort` 抽象暴露给 worker。

## 10. Future Cost/Pricing

战略层保留 Cost/Pricing 为 Future 能力，但本期只写约束：

- Future Cost 必须从 Usage 派生；
- 不得反向修改 Usage 事实；
- 不得使用未知模型的隐式 fallback 价格；
- 是否保存 PricingSnapshot、历史 Cost 是否重算，必须另行决策；
- 任何临时 Cost 实现都不是本期目标模型，迁移和退役统一记录在 Migration Governance。

## 11. 不变量

- **MUST** Runtime 写 Usage 时非阻塞。
- **MUST** Usage 只含 metadata，不含原文和 Cost。
- **MUST** 文档化 Accepted 到 flush 完成之间可能静默丢失的窗口；该窗口属于尽力审计语义，不得反向阻塞 Runtime。
- **MUST** 使用 Run/RunStep/ModelInvocation 统一 ID。
- **MUST** Usage 文件独立于 Session 存储。
- **MUST** Session 删除不级联 Audit。
- **MUST** 每条 append 后 flush。
- **MUST NOT** Audit 直接访问 filesystem。
- **MUST NOT** Audit 失败影响 Run 状态。
- **MUST NOT** CLI/TUI 直接解析 JSONL。

## 12. 相关文档

- Usage 持久化细节：[01-usage-storage.md](01-usage-storage.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Migration：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Usage-only Audit MVP、非阻塞 Sink、查询与独立 JSONL 分区 | #790 |
| 2026-07-15 | UsageSink 改为只引用 Runtime-owned trait，不重复定义签名；UsageAppendStorePort 明确由 Audit adapter 直接以 file append detail 实现（不复用 Storage 整值替换端口）；轮转拆分为 Audit 决策 + adapter 执行 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
