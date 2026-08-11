# Audit · Usage 持久化与恢复

> 层级：02-modules / audit（机制集成）
> 状态：Current（已实现并通过分层测试审查）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> Usage 是独立 Audit 事实流；SessionId 只是分区键，不改变 BC 所有权。

## 1. 文件布局

默认 File AppendLog Adapter：

```text
~/.agents/audit/
└── usage/
    ├── {session-id-a}.jsonl
    └── {session-id-b}.jsonl
```

每行是一个带 schema version 的 envelope：

```json
{"schema_version":1,"record":{"recorded_at_unix_ms":1720000000000,"session_id":"...","run_id":"...","run_step_id":"...","model_invocation_id":"...","provider":"...","model":"...","input_tokens":123,"output_tokens":45}}
```

Envelope 版本属于 Audit schema；#927 定义 `UsageEnvelopeV1 { schema_version: 1, record: UsageRecord }` 的稳定 serde 契约。Audit 的 File AppendLog Adapter 只读写 bytes 本身，不解释字段语义；#928 负责 append/read bytes，版本化 decoder 与字段级解析由 #930 reader 层实现。

## 2. 分区键

```text
UsagePartition = SessionId
```

选择按 canonical Main Session `SessionId` 分文件是查询与故障隔离策略，不表示 Usage 是 Session 聚合的一部分：

- 同一 Main Session 的 Main Run 与所有派生 Sub Run 使用同一分区文件；
- Main/Sub 记录以各自的 RunId / RunStepId / ModelInvocationId 区分；
- Session JSON 与 Audit JSONL 路径完全分离；
- Context Management 不持有 UsagePort；
- resume 不加载 Usage；
- 删除 Session 不触发 Usage 删除；
- RunId / RunStepId / ModelInvocationId 仍保留在每条记录中。

## 3. 写入流程

```text
Runtime
  → UsageSink.try_record(record)
  → bounded queue
  → Usage worker
  → serialize envelope + append exactly one trailing newline (#929 framing owner)
  → UsageAppendStorePort.append
  → UsageAppendStorePort.flush
```

每条 flush 的语义：worker 收到下一条记录前，上一条已调用 Audit adapter flush。默认 File AppendLog adapter 在 per-stream 进程内锁下执行 append `write_all`，flush 调用同一打开文件的 `sync_data`；因此 flush 成功表示已向 OS 请求同步该文件数据，且随后 reopen 可见完整行。它不承诺断电绝对持久、目录项 durability、跨进程全局顺序或 exactly-once。路径解析与 no-symlink 约束复用 Storage `SafeStorageRoot` / `SafePathSegment`，不经过 Storage 整值替换端口。

## 4. 顺序与重复

- 单个 worker 保持 dequeue 顺序；
- 默认 adapter 以 per-stream 进程内锁保证同进程 append/read/flush 互斥，完整行不交错；
- 同一 Session 分区内按 adapter 接受锁的顺序追加；
- 不承诺跨进程全局顺序；
- v0.1.0 不做去重和 exactly-once；
- `model_invocation_id` 为 Future 去重和诊断提供稳定关联；
- 调用方不得因不确定写入状态而重试 try_record，避免重复事实。

## 5. 损坏处理

读取 JSONL 时职责分层：

- Audit 的 File AppendLog Adapter 负责文件/IO 层错误隔离（复用 Storage 路径安全 primitive 解析路径，不经过 Storage 整值替换端口），并把字节行顺序交给 Audit reader；
- Audit reader 负责 JSONL schema 解析与行级损坏判断。

Audit reader：

1. 每行独立解析；
2. 损坏行报告结构化 `CorruptUsageLine { line_number }`；
3. 查询默认跳过损坏行并返回 warnings；
4. 一行损坏不得导致整个 Session 分区不可读；
5. 不自动重写或删除原文件。

截断尾行视为进程中断产生的损坏行，遵循同一策略。

## 6. Schema 演进

- 每行必须有 `schema_version`；
- reader 支持当前版本及明确列出的旧版本；
- 新增 optional 字段保持向后兼容；
- 重命名 ID 或 token 字段需要版本化 decoder；
- 不允许 Audit adapter 在 append/flush 路径上自行迁移 Audit schema（迁移只能发生在版本化 decoder 内）。

## 7. 查询

按 SessionId 查询只读取对应分区；目标分区尚未形成文件时返回空结果。v0.1.0 的 `AppendLogReader` 对单分区采用 eager `Vec<Vec<u8>>`，因此“流式”只表示逐分区处理、**NEVER** 同时加载全部 Session 文件；单个超大分区的真正 streaming reader 留作后续演进。跨 Session 查询：

- 由 Audit Query adapter 调用 `UsageAppendStorePort::list_streams("usage")` 枚举可用分区；
- 逐分区读取，不一次性加载全部 Session 文件；单分区 eager reader 是 v0.1.0 明确取舍；
- pagination 在 Audit BC 内实施：opaque V1 cursor 保存 query fingerprint、当前 stream 与下一条机械行偏移，续页从该位置重新应用全部 filter；分区目录在两页间发生外部变更时 cursor 失效并返回 `InvalidCursor`，不承诺跨列表快照的新增分区可见性；
- 每页最多返回 1000 条，调用方更大的 limit 被 policy clamp；
- token summary 在解析后聚合，跳过损坏行；
- 不计算 Cost。

## 8. 删除与 retention

v0.1.0：

- Session 删除保留 Usage；
- 不提供单条/单 Session Usage 删除命令；
- 不做自动 retention；
- 用户手工删除文件属于外部运维行为，查询应安全处理文件缺失。

Future retention 必须由 Audit Config 定义，并通过 Audit 的 File AppendLog Adapter（`UsageAppendStorePort` 之外新增的删除/归档能力，仍是 Audit-owned detail 实现，不经过 Storage OHS）执行；不能挂接 Session lifecycle 自动级联。

## 9. Schema 导入约束

若 Future 需要从其他 Usage 数据源导入记录：

- v0.1.0 不提供 importer；既有 `~/.agents/cost_history.json` 不读、不导入、不覆盖、不清空或删除；
- 只导入可验证的 raw token 字段；
- 忽略 cost / price 等派生字段；
- 缺少 RunId / RunStepId / ModelInvocationId 时不得伪造完整关联；
- importer 必须幂等、版本化，并有独立迁移标记；
- 具体旧格式与执行计划统一记录在 Migration Governance。

## 10. 验收场景

- [x] 两个 Session 写入不同文件。
- [x] Session 删除后 Usage 仍可查询。
- [x] 同一 Session 的多 Run/RunStep/Invocation 可分别过滤。
- [x] 每次 append 后调用 flush。
- [x] queue full 返回 Dropped，不阻塞 Runtime。
- [x] worker 写失败增加指标，不改变 Run。
- [x] 单行损坏不影响其余记录查询。
- [x] 截断尾行报告 warning。
- [x] 查询不返回 prompt/response/tool/hook 原文。
- [x] 查询结果不含 Cost/Price。

## 10. 测试完整性验收

### 10.1 行为—测试矩阵

| 行为 / 风险 | L0 | L1 | L2 | L3 | L4 | L5 | 最终证据 | 结论 |
|---|---|---|---|---|---|---|---|---|
| Usage PL、统一关联 ID、V1 envelope、敏感内容禁入 | crate/API/Cost 退役守卫 | summary 无 cost/price | Runtime factory 映射 | `tests/usage_contract.rs` | Runtime→Composition 落盘查询 | N/A | Usage contract；Runtime usage tests | 通过 |
| Session 分区与单行 framing | production reachability | decoder 终结行边界 | worker stream 映射 | append store 分区/payload | sender→worker→file→query | N/A | Audit application/adapter tests | 通过 |
| append、flush、reopen、no-follow | storage/架构守卫 | payload 边界 | append→flush 编排 | reopen、symlink、并发完整行 | 真实临时目录落盘查询 | N/A | `append_store_contract.rs` | 通过 |
| bounded sender、QueueFull、WorkerUnavailable | all-targets clippy | config 下界 | event-driven full queue/late reject | worker 公开契约 | Composition sink 透传 | N/A | `ingest_tests.rs`；worker contract | 通过 |
| worker FIFO、失败隔离、指标、drain | production-only check | config/metrics 边界 | ControlledStore append/flush 失败、FIFO、精确 timeout | worker API | worker→query、frontend drain | N/A | `ingest_tests.rs`；`chat_tests.rs` | 通过，无短 sleep |
| query 全字段过滤、半开范围 | production reachability | `validate_query_*`、`matches_*` | query service + store | 全关联字段契约 | 落盘后过滤查询 | N/A | query unit/contract tests | 通过 |
| cursor、跨分区续页、失效 | clippy/serde | round-trip、坏版本/hex/offset、fingerprint | query service 跨 stream | pagination contract | 文件分区续页 | N/A | query unit/contract tests | 通过 |
| 损坏 JSON、未知 schema、截断尾行、storage error | production reachability | decoder 精确 warning | adapter failing store | 坏行/未知版本契约 | 有效邻居保持可见 | N/A | adapter/query tests | 通过 |
| token summary、Cost/Price 禁入 | Cost retirement guard | optional token 累加 | query summarize | PL golden | worker→query summary | N/A | query tests；Cost guard | 通过 |
| Runtime logical invocation 只记录成功且 Dropped 不改写结果 | provider usage/Cost guards | UsageRecordFactory | 全字段与 Dropped | Runtime `UsageSink` | Composition 落盘 | N/A | Runtime usage tests | 通过 |
| Composition session worker、Main/Sub 共用 sink、canonical Session 分区 | construction/reachability guards | config snapshot 冻结 | context factory 共用 sink | sink outcome 透传 | shutdown 后完整 PL 查询 | N/A | Runtime factory；Composition assembly | 通过 |
| frontend drain 不覆盖结果；legacy Cost surface 不回流 | full guards | helper 结果正交 | 前端成功/失败均 drain 一次 | 私有 Future seam | CLI→SessionAudit shutdown | N/A | `chat_tests.rs`；Cost guards | 通过 |

L5 对本 MVP 判定为 **N/A**：真实文件系统、并发 append 和 shutdown 已由临时目录 adapter 契约及 L4 场景稳定覆盖；能力不依赖网络、PTY、安装资产、发布包或仅真实进程可触发的平台语义。CLI 既有 PTY smoke 不承担 Audit 行为断言。

### 10.2 执行叶子追溯

| 执行叶子 | 交付行为 |
|---|---|
| Usage contracts 与统一 ID | PL、关联 ID、V1 envelope、敏感内容边界 |
| Storage AppendLog | Session 分区、framing、append/flush/read/list、no-follow |
| bounded worker 与 shutdown drain | sender、worker、指标、失败隔离、drain |
| query/pagination/summary | filter、range、cursor、坏行、summary |
| Runtime/Composition 接线 | logical invocation、共享 sink、session worker、frontend drain |
| CostTracker 退役 | Runtime Cost/Pricing/DTO/history 不可达与 Guard |
| 目录收敛 | capability-oriented 目录和 owning-layer 测试归位 |

### 10.3 不符合项分类

- **文档错误**：页首仍标记 Target，但生产链路已完成；本节以实际代码与测试矩阵明确 Current 验收状态。
- **实现缺口**：未发现违反已批准 MVP 的业务实现缺口；CLI 仅增加私有 drain Future seam 以测试既有行为，不改变结果。
- **测试缺口**：已补 query L1、worker 事件驱动 L1/L2、storage error、unknown schema、落盘查询 L4、Runtime 全字段、Composition 完整 PL 和 frontend 结果正交测试。
- **过期测试**：已移除 worker contract 中依赖短 `sleep`、`>=` 和碰运气填满队列的测试；CLI inline tests 已迁至同层 `chat_tests.rs`。

### 10.4 确定性与组织结论

- Audit 测试不调用 `tokio::time::sleep` 或 `std::thread::sleep`。
- worker 使用 channel、Notify/Semaphore 等事件同步与 Tokio 虚拟时间。
- 文件测试使用独立 `tempfile::TempDir`，不修改 cwd 或进程环境。
- L1/L2 测试归 owning module 的 `*_tests.rs`；公开契约留在 crate `tests/`，没有万能 `test_utils`、`mod.rs`、inline tests 或 `include!`。
- Runtime、Composition、CLI 分别保留相邻边界测试，L4 不替代中间层证据。

### 10.5 验证与覆盖率

最终命令、首次失败和 line/region/function coverage 数值在完整门禁执行后回写。coverage 只作风险信号，production reachability 由独立 source guard 与 production-only check/clippy 证明。

## 11. 验收结论

十二个稳定行为单元已有 L0～L4 的必要证据，L5 有明确不适用理由；业务实现、测试、文档和旧路径退役边界一致。最终完成状态以 §10.5 全部门禁及覆盖率实测通过为准。

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-08-10 | 完成 Usage-only MVP 的 L0～L5 测试完整性审查：补齐 owning-layer 单元/协作、公开契约、真实落盘查询和 Runtime→Composition→CLI 相邻边界；L5 判定不适用 | 测试治理记录 |
| 2026-08-09 | #932 退役旧 Cost history 读写/path surface；既有 `cost_history.json` 保持用户 artifact，不自动导入、不覆盖、不删除，查询只读取 Audit Usage JSONL | [#932](https://github.com/rushsinging/aemeath/issues/932) |
| 2026-08-09 | #931 完成真实 Main/Sub logical invocation 写入同一 canonical Session Usage 分区与 SessionAudit drain | [#931](https://github.com/rushsinging/aemeath/issues/931) |
| 2026-07-21 | #930 完成版本化 V1 decoder、坏行/截断尾行 warning、关联字段与时间 filter、opaque cursor 续页和纯 token summary；缺失 Session 分区安全返回空 | [#930](https://github.com/rushsinging/aemeath/issues/930) |
| 2026-07-17 | #927 冻结嵌套 `UsageEnvelopeV1` serde 契约与责任边界：#928 只处理 bytes append/read，#930 实现版本化 decoder 与坏行处理 | [#927](https://github.com/rushsinging/aemeath/issues/927) |
| 2026-07-12 | 初稿：按 SessionId 分区的独立 Audit JSONL、逐条 flush 与损坏隔离 | #790 |
| 2026-07-15 | 修正职责归属：append/flush/IO 隔离/retention 执行改为 Audit-owned File AppendLog Adapter 直接实现，不再归 Storage | [#972](https://github.com/rushsinging/aemeath/issues/972) |
