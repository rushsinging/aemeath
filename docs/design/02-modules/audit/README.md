# Audit（支撑域）

> 层级：02-modules / audit（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> Audit MVP 只记录 Model Usage metadata；不保存原文，不计算 Cost，不阻塞 Runtime。
>
> 迁移基线：#988 已先行删除无行为的 `api/contract/gateway` COLA 占位。后续实现 **MUST** 依据真实 Usage 能力和 seam 增量建立 Hexagonal 层，**NEVER** 为目录对称恢复空层或 marker。

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
    recorded_at_unix_ms: u64,
    session_id: SessionId,
    run_id: RunId,
    run_step_id: RunStepId,
    model_invocation_id: ModelInvocationId,
    provider: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}
```

- `recorded_at_unix_ms` 是 UTC Unix 毫秒值；时钟由 Runtime 在构造事实时提供，本契约不读取系统时间；
- `SessionId` 语义归 Context Management，`RunId` / `RunStepId` / `ModelInvocationId` 语义归 Runtime；四者均通过 `packages/sdk` 发布唯一 UUIDv7 newtype，Audit 与 Context/Runtime 直接复用，**NEVER** 重复定义；
- provider / model 在 Usage schema 中保存标准化字符串，不复用 Provider 内部 `ModelId`，避免 Audit 对 Provider BC 形成依赖；
- Provider 在 ACL 内将 vendor wire usage 标准化为跨 BC `RawUsageSnapshot`（名称虽保留 Raw，语义已 provider-neutral，且表示单 attempt 累计快照）；Runtime 不解析供应商字段；
- 每个 `UsageRecord` 表示一个**成功完成且返回 usage 的逻辑 Model Invocation 聚合事实**，由 Runtime 在所有 retry/fallback attempt 收口后构造，恰好使用一个 `ModelInvocationId`；内部 attempt 可累加到最终 provider/model/token 结果，但不逐 attempt 写 Audit；
- 失败、取消或所有 attempt 均未返回 usage 时不伪造零值记录；对应运行诊断保留在 Runtime/Logging，不属于 Usage-only Audit fact；
- `provider` / `model` 记录最终成功产生 usage 的 provider/model；fallback 历史与 attempt ordinal 不进入 v0.1.0 Usage schema；
- SessionId 是分区和查询维度，不表示 Usage 属于 Session 聚合；
- 不冗余 parent_run_id，父子关系由 Run 模型解释；
- Provider 负责从供应商响应提取 raw usage，Runtime 在 Model Invocation 完成后构造 UsageRecord。

`UsageRecord` 由 Audit 拥有并作为 Published Language 发布；Runtime-owned `UsageSink` 只引用该类型，**NEVER** 在 Runtime 再定义同名 DTO。持久化使用 Audit-owned `UsageEnvelopeV1 { schema_version: 1, record: UsageRecord }`；#927 冻结 V1 serde 契约，#928 负责 append/read bytes，#930 负责版本化 decoder、坏行隔离与查询。

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

> `UsageSink` trait 由 **Runtime 拥有**并定义在 [runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)（`fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome`）。为避免 `runtime → audit`（消费 Audit PL）与 `audit → runtime` 形成 crate 循环，Audit crate **NEVER** 实现或依赖该 trait；#929 在 Audit 内发布具体、窄的 queue sender handle，#931 在 Composition Root 定义 bridge adapter，实现 Runtime-owned `UsageSink` 并委托该 handle。Audit 本文档 **NEVER** 重复定义 trait 签名，只发布该签名引用的结果类型：

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
- queue bounded；默认 `capacity = 1024`，`shutdown_timeout = 5s`。native global/project `aemeath.json` 使用 `audit.usage_queue_capacity` 与 `audit.usage_shutdown_timeout_ms`，字段级 patch 合并；Compatibility/Env/CLI/RuntimeOverride 本期不产出这两个字段。`Config::default()` 直接保存默认值；显式 0 作为兼容 sentinel，由 `ConfigSnapshot::usage_worker_config()` 回退默认。Composition 仅在进程 bootstrap 捕获一次 validated `UsageWorkerConfig { capacity, shutdown_timeout }` 并按值注入，运行期 Config commit 不重启 worker；Audit worker **NEVER** 持有 `ConfigReader`、`ConfigQuery` 或裸 Config 字段；
- `UsageSender::try_record` 的线性化点是 sender 内部短临界区：在同一 mutex 下检查 lifecycle 并执行 bounded `try_send`，**NEVER** await、序列化或 I/O；shutdown 获得该锁并切到 ShuttingDown 后，后续调用一律 WorkerUnavailable，已在线性化点前 Accepted 的记录属于 drain 集；
- metrics 使用同一 `UsagePipelineState` mutex 维护一致快照与单调 counter：`accepted_total` 在 enqueue 成功的同一临界区增加；`completed_total` 在该记录的 encode/append/flush 流程终结（成功或失败）后增加；`dropped_total{reason}` 在返回 Dropped 时增加；`write_failed_total` 每条记录至多增加一次并记录首个 encode/append/flush failure kind；`drain_abandoned_total` 在 timeout 时增加 `accepted_total - completed_total`，二者同锁读取、不会下溢；
- warning 分类固定为 `queue_full` / `worker_unavailable` / `encode` / `append` / `flush` / `drain_timeout`。每类累计计数从 0→1 及到达 64 的倍数时，在 `target: LOG_TARGET` 输出包含 kind 与 cumulative_total 的 warn；单条写流程首个失败后立即终结，不继续产生第二类写失败；drain_timeout 每次 shutdown timeout 立即 warn；
- enqueue Accepted 只表示进入队列，不表示 worker 已接收或已经落盘；Accepted 到 flush 完成之间存在进程异常导致的静默丢失窗口，这是尽力审计的明确语义。

`WorkerUnavailable` 仅在以下状态返回：Composition 尚未完成 worker 启动、worker 已不可恢复退出且 sender 关闭、或 graceful shutdown 已开始且不再接收新记录。

## 5. Worker 与写入语义

Usage worker 顺序消费 queue：

```text
receive one UsageRecord
  → encode UsageEnvelopeV1 as one newline-terminated bytes payload
  → UsageAppendStorePort.append(stream, bytes)
  → UsageAppendStorePort.flush(stream)
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

1. `UsageWorkerHandle::shutdown()` 使用启动时注入的唯一 `UsageWorkerConfig.shutdown_timeout`；在 sender 同一短临界区把 lifecycle 从 Running 置为 ShuttingDown 并移走 queue sender。重复 shutdown 等待/返回同一个 completion；
2. receiver 在 sender 关闭后 drain 已在线性化点前 Accepted 的记录；
3. worker 按 dequeue 顺序 encode，并 await `UsageAppendStorePort.append + flush`。默认 File adapter 自己在内部 `spawn_blocking` 执行同步 syscall，worker 不绕过 port；
4. handle 等待已注入的 shutdown timeout；
5. 超时时在一致 state 锁下计算 `accepted_total - completed_total`，增加 `drain_abandoned_total` 并返回 `TimedOut { unconfirmed }`。这里 abandoned 表示“timeout 时未确认完成”，不是确定丢失：已进入不可取消 blocking syscall 的单条可能稍后落盘。worker task 可 abort，但 blocking closure 不可取消；completion 保留该 unconfirmed 语义。

`Stopped` 包含自然 worker 退出、不可恢复 task failure 与 shutdown 完成；任何情况下 sender 检测非 Running 或 receiver closed 都返回 WorkerUnavailable。

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

struct AppendLogLine {
    bytes: Vec<u8>,
    terminated: bool,                // false = 文件尾未换行残片
}

struct AppendLogReader {
    lines: Vec<AppendLogLine>,       // 机械 bytes 行；不解析 Usage JSON
}

enum AppendLogError {
    Io,
    InvalidNamespace,
    InvalidStream,
    InvalidPayload,
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

Audit domain/application/worker 不直接访问 filesystem；Audit-owned File AppendLog **adapter MAY** 使用 Storage 发布的 `SafeStorageRoot` / `SafeStorageDir` 仅做 capability-root 下 no-follow 目录与普通文件句柄打开，并 **MUST** 以 `SafePathSegment` 解析 namespace 与 stream；`write_all` / `sync_data` / framing / read/list policy 均由 Audit adapter 执行，Storage primitive **NEVER** 定义 append 语义或消费整值替换 OHS。`append` 要求非空 bytes、内部不含 `\n` 且恰好以一个 `\n` 结束，违约返回 `InvalidPayload`；#929 worker 负责把序列化 envelope 封成该单行 payload。adapter 在 per-stream 进程内锁下单次 `write_all` 到 no-follow open-existing-or-create append 文件，绝不覆盖已有内容；`flush` 在同一锁下重新打开该 stream 并执行 `sync_data`。这建立“调用返回后已请求 OS file-data sync”的明确边界，但不承诺断电、目录项或跨进程 exactly-once。`read` 在同一锁下打开只读句柄并返回机械 bytes 行：已终结行剥离尾 `\n`，文件尾存在未终结残片时也作为最后一个元素原样返回，使 #930 可报告截断 warning；`list_streams` 只返回合法 `.jsonl` 普通文件、按 stream key 排序。`UsageAppendStorePort` 的方法保持 async 以服务 #929 worker；默认文件 adapter 当前执行同步文件 syscall，#929 **MUST** 在专用 blocking 边界调用（如 `spawn_blocking` 或专用线程），NEVER 在 Runtime hot path 或通用 async executor worker 上直接执行 `sync_data`。#928 以 contract harness、flush 后 reopen、symlink fail-closed 与 append/read/flush 互斥测试冻结该语义。

Audit adapter（detail 执行机制）负责：

- `SafeStorageRoot` 下的路径映射与目录创建；该 primitive 只提供 capability-root/no-symlink 机制，不拥有 append 语义；
- append + `sync_data` flush 的物理执行；
- per-stream 进程内锁与 handle 生命周期；跨进程顺序不在 MVP 承诺；
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

UsageAppendStorePort 是 Audit-owned 出站端口，#928 负责定义并实现，因为它的首个真实消费者/实现从 #928 开始、且 #929 worker 依赖该交付；adapter 直接以 file append detail 实现，不得命名为 SessionLogPort 或让 Storage 解释 Usage 字段。

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
    provider: Option<String>,
    model: Option<String>,
    recorded_range: Option<TimeRange>,
    pagination: Pagination,
}

struct TimeRange {
    from_inclusive_unix_ms: Option<u64>,
    to_exclusive_unix_ms: Option<u64>,
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
    Storage(String),             // query-facing 安全诊断；NEVER 暴露 adapter error 类型
    InvalidRange,
    InvalidCursor,
}

struct UsageQueryWarning {
    CorruptLine { stream: String, line_number: u64 }, // opaque stream label
}
```

`UsageQueryError` / `UsageQueryWarning` 是稳定的 query-facing PL，**NEVER** 引用 #928 才定义的 `AppendLogError` / `AppendLogStream` adapter 类型；#930 负责把内部错误与 stream key 防腐映射为安全字符串。`UsageCursor` 对调用方不透明，只用于稳定续页；#930 的 cursor 采用版本化 `query fingerprint + stream + next line offset` 内部编码，续页从该行重新应用同一过滤谓词，filter 改变、cursor 格式错误或 cursor 所在分区已被外部删除即返回 `InvalidCursor`，因此 append-only 分区内不丢不重。上限由 Audit query policy 统一 clamp 为 `1000` 条，避免单页失控而不改变查询语义。版本化 decoder 仅接受当前 `UsageEnvelopeV1`；JSON 解析失败、未知版本与未终结尾行统一跳过并返回 `CorruptLine` warning，append-store I/O 则映射为 `Storage`，不得伪装成坏行。`UsageSummary` 仅是 token/record_count 投影，遇到损坏行沿用同一 skip 规则但不携带 warnings；需要定位损坏行时调用 `query`。#927 只冻结上述 DTO 与 `UsageQueryPort` 签名，不实现文件扫描、filter、cursor、坏行处理或 token 聚合。

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
2. 调用 Audit #929 factory 创建 bounded queue、Usage worker 与不依赖 Runtime trait 的具体 sender handle；
3. 在 Composition crate 定义 bridge adapter，实现 Runtime-owned `UsageSink` 并委托 Audit sender handle，再向 Runtime 注入 `Arc<dyn UsageSink>`；
4. 向 CLI/TUI/Server 查询用例提供 UsageQueryPort；
5. 负责 shutdown drain/flush。

依赖方向：

```text
Runtime → Audit PL + Runtime-owned UsageSink
Composition bridge → Runtime UsageSink + Audit sender handle
Audit worker → UsageAppendStorePort
Audit File AppendLog adapter → Storage path-safety PL + constrained filesystem detail
CLI/TUI → UsageQueryPort → Audit
```

Audit domain/application/worker 不依赖 Runtime、TUI、Logging 具体实现或直接拼接文件路径；实际文件系统访问只在 Audit-owned File AppendLog adapter 内，并依赖 Storage path-safety PL。`runtime → audit` 只消费 Audit PL，Composition 同时依赖两者并拥有 bridge impl，从物理依赖上禁止 `audit → runtime` 回边。

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
- **MUST NOT** Audit domain/application/worker 直接访问 filesystem；文件访问只允许在 Audit-owned adapter，并受 Storage path-safety PL 约束。
- **MUST NOT** Audit 失败影响 Run 状态。
- **MUST NOT** CLI/TUI 直接解析 JSONL。

## 12. Target 物理目录

Audit 采用 Hexagonal + Clean 组织（`domain + application + ports + adapters` 按证据增量出现）。v0.1.0 Audit 只拥有 Usage 能力：UsageRecord schema、分区策略与编码语义收在 `domain`；queue 消费策略、append+flush 编排、shutdown drain 与 query policy 收在 `application`；对外端口定义在 `ports`；具体 channel、文件 IO 与扫描 detail 终止在 `adapters`：

```text
src/
├── lib.rs                   # 窄 façade：Audit PL + UsageQueryPort
├── domain.rs                # 领域策略入口（#927 起）
├── domain/
│   └── usage.rs             #   UsageRecord / V1 envelope / emit 与 query PL
├── ports.rs                 # UsageQueryPort（#927 起）；后续按真实端口扩展
├── application/             # #929/#930 出现真实用例时建立
│   ├── ingest.rs            #   #929 bounded queue + Usage worker + shutdown drain
│   └── query.rs             #   #930 filter / cursor / summary / decoder
└── adapters/                # 对应技术实现出现时按 Issue 增量建立
    ├── append.rs            #   #928 File AppendLog adapter（UsageAppendStorePort 实现）
    ├── append/              #   #928 仅在路径映射 / 轮转 / IO 错误隔离已独立变化时展开
    └── query.rs             #   #930 query 文件扫描实现（仅在技术扫描独立变化时展开）
```

`domain/usage.rs` 承载 schema、分区策略与编码语义等 Audit 业务决策；轮转是 Audit 决策、`adapters/append.rs` adapter 只执行，两者 **NEVER** 混位。文件 IO detail、worker 内部状态和 adapter 实现细节 **NEVER** 泄漏到 façade 之外。单文件即可讲清时 **MUST** 保持为 `.rs` 文件而非空壳目录。Cost/Pricing 作为 Future 能力出现时，若证明与 Usage 拥有独立词汇与变化原因，才 **MAY** 重新评估竖切。

## 13. 相关文档

- Usage 持久化细节：[01-usage-storage.md](01-usage-storage.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Migration：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-21 | #930 实现 Audit-owned `UsageQueryPort`：按关联 ID/provider/model/半开时间范围过滤、版本化 opaque cursor、单页 1000 条 clamp、逐行 V1 decoder、损坏行/截断尾行 warning 与纯 token summary；查询仅经 `UsageAppendStorePort` 逐分区读取，CLI/TUI 不解析 JSONL，生产 wiring 仍归 #931 | [#930](https://github.com/rushsinging/aemeath/issues/930) |
| 2026-07-18 | #929 冻结 bounded sender/worker lifecycle：默认 capacity 1024、shutdown 5s；一致 state metrics、64 倍数聚合 warning、File adapter blocking boundary、超时 unconfirmed 计数与 Composition lifecycle assembly；Runtime bridge/Invocation wiring 仍归 #931 | [#929](https://github.com/rushsinging/aemeath/issues/929) |
| 2026-07-17 | #927 冻结 Usage PL：跨 BC ID 复用 SDK 唯一 newtype，Audit 拥有 UsageRecord/V1 envelope/emit/query DTO，Runtime 仅拥有 UsageSink trait；配置归 #929，查询行为归 #930，并按真实交付增量建立 domain/ports 层 | [#927](https://github.com/rushsinging/aemeath/issues/927) |
| 2026-07-17 | #988 删除无行为的 `api/contract/gateway` COLA 占位；Usage 实现前仅保留真实 crate 入口，后续按已冻结的 Usage Target 增量建层 | [#988](https://github.com/rushsinging/aemeath/issues/988) |
| 2026-07-12 | 初稿：Usage-only Audit MVP、非阻塞 Sink、查询与独立 JSONL 分区 | #790 |
| 2026-07-15 | UsageSink 改为只引用 Runtime-owned trait，不重复定义签名；UsageAppendStorePort 明确由 Audit adapter 直接以 file append detail 实现（不复用 Storage 整值替换端口）；轮转拆分为 Audit 决策 + adapter 执行 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | 冻结 Audit Target 物理目录：扁平 Usage 能力 + `ingest`（queue/worker/drain）、`append`（File AppendLog adapter）、`query`（扫描/聚合/续页）技术实现；明确不建 `capabilities/`（v0.1.0 只拥有 Usage，无独立业务切片） | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
