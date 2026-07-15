# Audit · Usage 持久化与恢复

> 层级：02-modules / audit（机制集成）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
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
{"schema_version":1,"recorded_at":"...","session_id":"...","run_id":"...","run_step_id":"...","model_invocation_id":"...","provider":"...","model":"...","input_tokens":123,"output_tokens":45}
```

Envelope 版本属于 Audit schema；Audit 的 File AppendLog Adapter 只读写 bytes 本身，不解释字段语义（字段级解析在 §5 reader 层进行）。

## 2. 分区键

```text
UsagePartition = SessionId
```

选择按 SessionId 分文件是查询与故障隔离策略，不表示 Usage 是 Session 聚合的一部分：

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
  → serialize envelope as one JSON line
  → UsageAppendStorePort.append
  → UsageAppendStorePort.flush
```

每条 flush 的语义：worker 收到下一条记录前，上一条已请求 Audit adapter flush。它不等同于 fsync 或绝对持久性；具体 flush/fsync 语义由 Audit 的 File AppendLog Adapter 以 file append detail（而非 Storage 整值替换协议）自行定义并执行，只复用 Storage 发布的路径安全 primitive。

## 4. 顺序与重复

- 单个 worker 保持 dequeue 顺序；
- 同一 Session 分区内按 worker 接收顺序追加；
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

按 SessionId 查询只读取对应分区。跨 Session 查询：

- 由 Audit Query adapter 调用 `UsageAppendStorePort::list_streams("usage")` 枚举可用分区；
- 流式读取，不一次性加载全部文件；
- pagination 在 Audit BC 内实施；
- token summary 在解析后聚合；
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

- 只导入可验证的 raw token 字段；
- 忽略 cost / price 等派生字段；
- 缺少 RunId / RunStepId / ModelInvocationId 时不得伪造完整关联；
- importer 必须幂等、版本化，并有独立迁移标记；
- 具体旧格式与执行计划统一记录在 Migration Governance。

## 10. 验收场景

- [ ] 两个 Session 写入不同文件。
- [ ] Session 删除后 Usage 仍可查询。
- [ ] 同一 Session 的多 Run/RunStep/Invocation 可分别过滤。
- [ ] 每次 append 后调用 flush。
- [ ] queue full 返回 Dropped，不阻塞 Runtime。
- [ ] worker 写失败增加指标，不改变 Run。
- [ ] 单行损坏不影响其余记录查询。
- [ ] 截断尾行报告 warning。
- [ ] 查询不返回 prompt/response/tool/hook 原文。
- [ ] 查询结果不含 Cost/Price。

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：按 SessionId 分区的独立 Audit JSONL、逐条 flush 与损坏隔离 | #790 |
| 2026-07-15 | 修正职责归属：append/flush/IO 隔离/retention 执行改为 Audit-owned File AppendLog Adapter 直接实现，不再归 Storage | [#972](https://github.com/rushsinging/aemeath/issues/972) |
