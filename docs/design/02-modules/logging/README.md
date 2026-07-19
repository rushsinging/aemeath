# Logging（通用域）

> 层级：02-modules / logging（模块摘要设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#793（S2）
> Logging 提供 best-effort 诊断日志的 schema、过滤、路由、sink、rotation 与 retention 机制；它不拥有业务事件，也不承担 Audit 的不可变审计语义。

## 1. 模块定位

```text
各 BC / 交付层
   │ DiagnosticRecord + LogContext
   ▼
Logging Facade
   ├── FilterPolicy
   ├── TargetCatalog → Sink Route
   ├── 14-field JSONL Formatter
   └── Diagnostic Sink + Rotation

Audit Event ───────────────▶ AuditSink（不经过 Logging 领域模型）
```

Logging 回答“怎样记录诊断信息”，业务 BC 决定“什么行为值得记录”。日志中的 run/session/request 标识只是关联信息，不代表 Logging 拥有这些聚合。

## 2. 核心决策

1. **诊断与审计分离**：Logging 是可降级、best-effort 的诊断机制；Audit Event、Cost/Usage、不可变 sink 与审计 retention 归 Audit BC。
2. **14 字段 schema 是稳定契约**：所有诊断 sink 使用同一 compact JSON Lines schema；字段增删必须版本化评审，不能由单一调用点私自扩展。
3. **TargetCatalog 是单一真相**：合法 target、owner、sink 文件和匹配规则只定义一次，formatter、router、guard 与文档引用同一 catalog。
4. **最长前缀路由**：合法 target 路由到专属 sink；未注册 target 进入 `aemeath.log` 硬兜底，并产生可观察诊断。
5. **上下文按执行 scope 传播**：Main/Sub 并发相关字段通过显式 `LogContext` 或 task-local scope 绑定，禁止使用进程级可变 current 值。
6. **进程元数据与调用上下文分离**：`boot_ts/pid/ver` 可全局只读；`session/chat/turn/request_id/model/provider/role` 必须随当前异步执行 scope 传播并自动恢复。
7. **sink 失败不静默**：文件写入、flush、rotation、reopen 失败时按输出模式降级；File 模式写 `emergency.log`，Stderr 模式直写 stderr。失败 sink 保持可恢复状态，不能永久变为 None 而无信号。
8. **配置来自 ConfigSnapshot**：level、输出模式、目录、max bytes、backup 数和 retention days 由 Config 提供静态值；Logging 执行过滤和轮转机制。
9. **敏感内容默认不记录**：级别、preview 与脱敏规则先于格式化应用；Logging 不因 trace 开启而允许 API key、认证 header 或未清洗 secret 泄漏。
10. **调用方不硬编码 target**：每个 crate 引用注册的 target 常量；架构守卫检查裸日志调用与未知 target。
11. **LLM API Error 独立诊断 sink**：Provider HTTP/网络/协议/流错误使用注册 target `aemeath:llm-api-error`，路由到 `llm-api-error.log`。它仍是 14 字段 best-effort DiagnosticRecord，不是 Audit；`msg` 使用受控 JSON payload，先由 Provider 生成脱敏事实，再由 Logging 机械路由。

## 3. 14 字段 DiagnosticRecord

| # | 字段 | 类型 | 语义 |
|---|---|---|---|
| 1 | `ts` | string | 本地 RFC3339 时间，毫秒精度 |
| 2 | `boot_ts` | string/null | 进程启动时间 |
| 3 | `pid` | number | 进程 ID |
| 4 | `ver` | string/null | aemeath 应用版本 |
| 5 | `session` | string | Session ID；未知为 `-` |
| 6 | `chat` | string | Chat/Segment 关联 ID；未知为 `-` |
| 7 | `turn` | number/null | turn 编号 |
| 8 | `request_id` | string/null | 单次外部请求关联 ID |
| 9 | `model` | string | model ID；未知为 `-` |
| 10 | `provider` | string/null | provider identity |
| 11 | `role` | string/null | Main/Sub role |
| 12 | `level` | string | ERROR/WARN/INFO/DEBUG/TRACE |
| 13 | `target` | string | 注册 target |
| 14 | `msg` | string/null | 已应用级别、preview 与脱敏策略的消息 |

14 字段是 v1 兼容基线；`ver` 仍表示应用版本，不充当 schema version。需要通用 attributes、`run_id/agent_id/span_id` 或 error kind 时，必须设计 v2 schema（例如新增 `schema_ver` 与受控 attributes），不能把机器查询字段长期编码进自由文本 `msg`。v1 期间并发 Sub 必须通过唯一 chat/request 关联；无法满足时应升级 schema，而不是复用 role 作为唯一身份。

### 3.1 LogContext

```rust
struct LogContext { /* 7 个执行字段的已解析快照 */ }

struct LogContextPatch {
    session_id: FieldPatch<SessionId>,
    chat_id: FieldPatch<ChatId>,
    turn: FieldPatch<TurnNumber>,
    request_id: FieldPatch<RequestId>,
    model: FieldPatch<ModelId>,
    provider: FieldPatch<ProviderId>,
    role: FieldPatch<AgentRole>,
}

enum FieldPatch<T> { Inherit, Set(T), Clear }

trait LogScope {
    fn capture(&self) -> LogContext;
    async fn within<T>(&self, patch: LogContextPatch, future: impl Future<Output = T>) -> T;
    fn instrument<T>(&self, context: LogContext, task: T) -> Instrumented<T>;
}
```

目标语义：

- child scope 默认继承父 context，`Set/Clear` 显式覆盖或清空；
- scope 结束自动恢复父 context；
- Main 与多个 Sub task 的 context 互不覆盖；
- 新 request 创建 child scope，而不是改写进程全局；
- spawn 新 task 时先 `capture`，再 `instrument` 绑定；禁止依赖运行时默认继承；
- 同步线程路径把捕获的 LogContext 显式传给 record builder；不把 Tokio 类型暴露为模块契约。

## 4. TargetCatalog 与路由

```rust
struct TargetSpec {
    target: LogTarget,
    owner: ModuleOwner,
    sink: DiagnosticSinkId,
}
```

目标 catalog 覆盖具有独立运行时边界的 owner：CLI、Composition、Shared 及各业务 feature crate；每个 owner 都有独立 target、sink ID 与日志文件，并在真实架构入口和终态记录低频、脱敏日志。Provider 另保留 LLM API Error 专用 target，Runtime 的 Prompt target 作为专用子能力路由。纯契约 SDK、纯函数 Utils、Logging 自身和未接入 UnifiedLogger 的 xtask 不制造应用 target；其边界分别由执行 owner、direct emergency diagnostics 或 CLI 输出负责。Audit Event 仍走独立 `AuditSink`。

路由规则：

1. target 必须以 `aemeath:` 开头；
2. 完全匹配或合法子 target 按最长前缀命中；
3. target 与 sink 文件映射来自同一 catalog；
4. 未注册 target 写 `aemeath.log`；未知 target 的限频诊断也写同一 fallback sink，**NEVER** 旁路到 stderr；
5. sink 文件名必须唯一；
6. catalog guard 从根 `Cargo.toml` 的 workspace members 反向校验 runtime owner 与已登记的非 runtime member：runtime owner 的 crate root 恰有一个 crate-private `LOG_TARGET`，target、sink ID 与文件名唯一；非 runtime member 禁止 target、匿名保活及不必要的 Logging 依赖；同时扫描全部生产 `log::xxx!` 调用。

## 5. 级别、preview 与脱敏

| Level | 用途 | 内容原则 |
|---|---|---|
| Error | 最终不可恢复失败 | 不含 secret；说明影响与安全错误类别 |
| Warn | 可恢复异常/降级 | 重试只在最终汇总告警 |
| Info | 低频生命周期 | 只记录结构化元数据，不记录完整 payload |
| Debug | 中粒度诊断 | 安全截断 preview |
| Trace | 高频协议/流细节 | 仍不得记录 secret 或未经清洗的敏感 body |

Logger 可提供安全 preview helper，但业务 BC 仍负责识别自身敏感字段。完整 LLM I/O、Tool arguments 或用户输入不是默认诊断内容；需要记录时必须经过专门的脱敏和配置门禁。

### 5.1 LLM API Error payload 与脱敏边界

`aemeath:llm-api-error` 的 `msg` 是 compact JSON object，`event_type="llm_api_error"`。Provider 应尽可能提供：driver/API、provider/model、request correlation、只含 scheme/host/path 的 endpoint、method/status、provider request ID、error kind/code、retryable、attempt/max attempts、retry-after、elapsed、message/tool/request/response 字节统计、partial-output 标志、受限 body preview 与 source chain。

- **NEVER** 写 API key、Authorization/Cookie、完整 URL query、完整 prompt/messages/tool args 或完整 request body。
- endpoint 必须移除 userinfo、query 和 fragment；body/source preview 必须限长并对 `api_key/access_token/authorization/cookie/password/secret/token` 等常见 key/value 脱敏。
- 取消是正常控制流，**NEVER** 写入 LLM API Error sink。
- 可重试中间失败用 `debug`；不可重试或重试耗尽用 `error`。同一次 attempt 的同一失败只写一条，fallback 前的失败必须标明 `fallback_planned`，避免误判为最终失败。
- payload 构造、endpoint 清洗与 preview 脱敏归 Provider；Logging 只负责 route/format/rotation，不解析 vendor body，也不拥有错误分类。


每个 sink 独立串行化写入，避免跨文件全局锁。文件模式使用 compact JSONL，单条 record 一行。

```rust
// Adapter-private fault-injection seam; not a public BC port.
trait SinkWriter: Send { /* write_all + flush */ }
trait FileOps: Send + Sync { /* open/metadata/exists/remove/rename/read_dir */ }
trait MonotonicClock: Send + Sync { /* now */ }
trait EmergencyWriter: Send + Sync { /* direct write */ }

struct FileSinkLifecycle {
    state: SinkState,
    writer: Option<FileWriter>,
    rotation: RotationPolicy,
    recovery: RecoveryPolicy,
}

enum SinkState { Healthy, Degraded, Recovering }
```

`RotationPolicy` 只判断阈值和保留参数；`FileSinkLifecycle` 在单一 sink 锁内拥有 flush → close → rename → reopen 的机械流程，避免 policy 接口假装操作看不到的 writer。

不变量：

- 同步 facade 的写入预算必须有界；v1 允许锁内直接写文件，但只用于短小 record；若改为异步队列，必须另行定义队列满时的 drop/backpressure、flush 和 shutdown 语义；
- rotation 在该 sink 的写锁内完成，不能重入同一非可重入锁；
- rotate 前 flush，成功后重开 writer；
- 本次写失败的 record 立即交给当前输出模式的 emergency writer；File 模式写 `emergency.log`，Stderr 模式直写 stderr，不承诺重放到目标文件；
- 重开失败进入 Degraded，后续 record 继续走 emergency writer，并按 RecoveryPolicy 限频尝试恢复；
- file write/flush/rename/remove 错误不得被吞掉；
- fallback 自身不能递归调用 Logging；File emergency 打开失败时 best-effort 静默丢弃，**NEVER** 再回退 stderr 污染 TUI；
- 同一健康 sink 的文件写顺序保持，跨 sink不承诺全局顺序；
- retention 只删除符合本模块轮转命名协议的文件；
- rotation 和 retention 静态参数来自 ConfigSnapshot。

#939 冻结并实现以下 v1 语义：

- 生命周期转换为 `Healthy --I/O failure--> Degraded`；`Degraded` 在同步写入口惰性恢复，到期首条 record 瞬时进入 `Recovering` 并只尝试一次 reopen；成功回到 `Healthy` 并写入该 record，失败回到 `Degraded`；
- recovery interval 固定为 5 秒并使用单调时钟；无后台线程、指数退避或历史 record 重放；截止时间前的每条 record 仍直接写当前输出模式的 emergency sink；
- startup 时某个 sink 的 open 失败只降级该 sink，不阻止其他 sink 或全局 logger 安装；日志根目录创建失败仍是整体初始化错误；
- adapter-private `FileOps`、`SinkWriter`、`MonotonicClock`、`EmergencyWriter` 是 fault-injection seam，不进入公共 façade/port；open、write、flush、metadata、backup existence、remove、rename、rotation reopen 与 recovery reopen 故障均报告到当前 emergency sink，且 `exists` 保留 `io::Result<bool>`、不得把 `try_exists` 错误吞成不存在；
- 每个 sink 持有独立 mutex；一个 sink 的慢 I/O 或故障不占用其他 sink 的锁；显式 `Log::flush` 为 best-effort，flush 故障使对应 sink 降级并报告；全局 shutdown API 仍不在 v1 范围；
- `max_bytes=0` 在 `LoggingSettings` 边界归一化为 1；`max_backups=0` 在轮转时删除 active 并重建空 active；
- `retention_days=0` 禁用按天清理；非零时在初始化与每次成功轮转后清理 active 同目录、同 basename、非空数字 `.log.N` 后缀且为普通非 symlink 的过期 backup；其他 basename、目录、非法后缀、目录和 symlink 均不删除。retention 的 metadata/remove 故障直接报告但不破坏已恢复的健康 writer。

Logging 是 best-effort：诊断 sink 失败不能自动阻断 Run；但失败必须可观察。需要“写失败就阻断”的数据不得走 Logging，应使用对应领域端口，例如 `AuditSink`。

## 7. Audit 边界

| Logging | Audit |
|---|---|
| 诊断 record | 不可变 Audit Event |
| best-effort，按输出模式降级到 `emergency.log` 或 stderr | durability/append-only 由 Audit 策略决定 |
| 14 字段诊断 schema | 独立审计 PL/schema |
| target 路由与日志文件 | AuditSink 与查询投影 |
| 不拥有 Cost/Usage | 拥有 Pricing、Cost、Usage 聚合 |
| 不影响业务控制流 | 同样不驱动 Runtime，但失败语义由 Audit 设计 |

本摘要只锁定边界：Audit Event 绝不序列化为 DiagnosticRecord，AuditSink 绝不通过 target 文件路由伪装。Audit Usage Fact 只走 Audit-owned append store；Audit 模块自身的 queue/write/drain 故障属于普通 DiagnosticRecord，使用显式 `aemeath:diagnostic:audit` → `audit-diagnostic.log`，其 payload 不含 `UsageRecord`/`UsageEnvelopeV1`。两条数据流从名称、schema、sink 与 Guard 上均分离。

## 8. Composition Root

Composition Root 负责：

- 从 ConfigSnapshot 的细粒度 accessor 构造不可变 `LoggingSettings`；该值完整持有 filter directive、max level、输出模式、日志目录、rotation 与 retention 静态参数；
- filter 与 `log::set_max_level` 从同一 directive 单次归一化，Logging 内部不再读取 env；
- 自定义 `logs_dir` 优先，未配置时由 Composition 提供全局默认目录；
- 在业务执行开始前初始化一次全局日志 facade；
- 为 Runtime/Provider/Tool 等执行 scope 提供 LogContext 适配；
- 注册按输出模式选择的 emergency writer：File → `emergency.log`，Stderr → direct stderr；
- 不让消费方直接构造具体文件 sink；
- 配置热更新若受全局 logger 限制，应通过可更新 policy handle 实现，而不是重复初始化 logger。

## 9. 架构守卫目标

```text
Rule: logging-targets-come-from-catalog
Deny: unknown target literals and bare log macros

Rule: logging-context-is-scope-local
Deny: mutable process-global current request/model/provider/role state

Rule: audit-events-do-not-use-diagnostic-log-contract
Deny: Audit Event serialization through DiagnosticRecord/target routing
```

## 10. Target 物理目录

Logging 采用 Hexagonal + Clean 组织（`domain + adapters`）。诊断记录流水线的领域策略（14 字段 schema、FilterPolicy、TargetCatalog 路由规则）收在 `domain`；文件 sink、rotation 与 retention 技术实现在 `adapters`：

```text
src/
├── lib.rs                 # 窄 façade：DiagnosticRecord + record 入口 + composition-only wiring
├── domain.rs              # 领域策略入口
├── domain/
│   ├── schema.rs           #   14 字段 DiagnosticRecord + LogContext / LogScope
│   ├── filter.rs           #   FilterPolicy + 级别 / preview / 脱敏策略
│   └── routing.rs          #   TargetCatalog + 最长前缀路由 + target 校验规则
└── adapters/
    ├── file_sink.rs        #   FileSinkLifecycle + adapter-private fault seam + mode-aware emergency
    └── lifecycle.rs        #   rotation + retention + recovery
```

每个阶段是同一条 `DiagnosticRecord → 过滤 → 路由 → 写入 → 轮转` 管线的一个环节。`domain` 定义 schema、过滤策略和路由规则；`adapters` 实现具体的文件写入、rotation 与 retention 机械流程。各文件 **MUST** 私有，只通过 façade 暴露 `DiagnosticRecord` 与 `LogScope`；file writer 句柄、rotation 机械流程和 target wire type **NEVER** 泄漏到 façade 之外。跨阶段共享的 14 字段契约由 `domain/schema.rs` 唯一定义，**NEVER** 在其他文件重复字段定义。

## 11. 测试分层与验收证据

| 行为 / 风险 | 层级 | 可追溯证据 | 结论 |
|---|---|---|---|
| 14 字段 schema、空值与 compact JSONL | L1/L3 | `adapters/formatter_tests.rs` | 字段集合、显式 context、无 scope 默认值与单行格式均覆盖 |
| `FieldPatch` 的 Inherit/Set/Clear | L1 | `domain/context_tests.rs` | 父快照不可变与默认继承覆盖 |
| task-local 嵌套、并发、取消、panic、spawn 传播 | L2 | `adapters/context_scope_tests.rs` | scope 恢复和并发隔离覆盖 |
| Main/Sub/physical request 与 retry request ID | L2/L4 | Runtime `application/agent/runner/tests.rs`、`application/chat/looping/*_tests.rs` | Main/Sub 相邻边界、并发角色与 retry ID 覆盖 |
| Provider blocking stream bridge | L3/L4 | Provider `adapters/stream.rs` 的 producer/event/consumer context 场景 | 同一不可变 context 跨同步 bridge 完整传播 |
| TargetCatalog 唯一性、最长合法前缀与 fallback | L1/L3 | `domain/routing_tests.rs`、`domain/routing_guard_tests.rs` | target/sink/file 唯一和 fail-closed 边界覆盖 |
| Audit Fact 与诊断路由隔离 | L0/L3 | `audit_facts_have_no_diagnostic_route`、`audit_facts_are_forbidden_from_diagnostic_catalog`、target guard | 旧 target/file 被拒绝，Audit 运行诊断使用独立 target/file |
| File/Stderr output mode 与 emergency 行为 | L2/L4 | `adapters/file_sink_tests.rs`、CLI `args.rs` output mapping tests | File 模式写 `emergency.log` 且不污染 TUI；no-TUI verbose 保留 stderr |
| sink degrade、5 秒惰性恢复、rotation、retention 与跨 sink 隔离 | L2/L3 | `adapters/file_sink_fault_tests.rs`、`adapters/lifecycle_tests.rs` | 注入 clock/file ops 覆盖完整故障矩阵，无短 sleep |
| ConfigSnapshot → LoggingSettings → Composition 唯一初始化 | L0/L2 | `domain/settings_tests.rs`、Composition/Runtime 定向测试、`check-logging-settings-injection.sh` | 无 env 旁路，Runtime 不装配 Logging |
| owner-aware target、scope-local context、production reachability | L0 | 三个 Logging Guard、`check-production-reachability.sh`、workspace clippy | 正反向规则与生产可达性由总架构门禁执行 |
| 真实网络、PTY、发布资产 | L5 | 不适用 | Logging 机制可由注入 seam 和 CLI/TUI 场景确定性覆盖，不新增易 flaky smoke |

#1066 审查基线：测试外置后的 Logging 生产源码口径为 line 86.82%、region 85.81%、function 86.75%（外置前测试与生产同文件的混合口径为 88.02% / 87.44% / 87.80%，不可直接横向比较）；百分比只作为风险信号，最终结论以上述行为矩阵、fault matrix 与跨层证据为准。Logging 自身的历史内嵌测试已迁到同级 `*_tests.rs`。审查同时发现全仓 `check-no-inline-tests.sh` 检测器失效及大量跨 crate 存量，由 [#1240](https://github.com/rushsinging/aemeath/issues/1240) 独立承接，不把全仓迁移混入本模块验收。首次 `cargo test --workspace` 失败于与 Logging 无关的 Runtime canonical tool name 旧断言，已有 [#1238](https://github.com/rushsinging/aemeath/issues/1238) / PR #1242 承接；本审查保留首次失败，不以重跑覆盖。

## 12. 相关文档

- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Provider 可观测性边界：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-19 | #1066 完成父项级 L0–L5 测试审查：建立行为—证据矩阵，记录 Logging crate 覆盖率基线，迁出 Logging 内嵌测试；同步 File/Stderr emergency、Audit diagnostic target 与 request scope 的真实语义。全仓测试组织守卫缺口由 #1240 承接 | [#1066](https://github.com/rushsinging/aemeath/issues/1066) |
| 2026-07-19 | 删除 legacy 全局执行上下文、setter/getter 与 formatter fallback；Audit Usage Fact 保持独立 append store，Audit 模块运行诊断改用明确 diagnostic target/file，并由 Guard 禁止旧 Audit 路由回流 | [#942](https://github.com/rushsinging/aemeath/issues/942) |
| 2026-07-18 | CLI 通过 typed bootstrap input 选择 File/Stderr，Composition 保持 Runtime 前唯一初始化；TUI 使用 session-only delivery scope；全仓生产日志迁到 owner 常量并启用 owner-aware Target Guard | [#941](https://github.com/rushsinging/aemeath/issues/941) |
| 2026-07-18 | Main/Sub 生产链按 session→chat/sub-run→turn→physical request 建立不可变 scope；Runtime task 与 Provider blocking stream bridge 显式传播 opaque `LogContext`，legacy 全局状态最终退役仍由 #942 承接 | [#940](https://github.com/rushsinging/aemeath/issues/940) |
| 2026-07-18 | 实现可恢复 FileSinkLifecycle：per-sink lock、5 秒惰性 reopen、mode-aware emergency、完整 I/O fault seam、rotation/retention 与 max-bytes 边界语义；#1215 后 File 模式 emergency 固定写 `emergency.log` | [#939](https://github.com/rushsinging/aemeath/issues/939) / [#1215](https://github.com/rushsinging/aemeath/issues/1215) |
| 2026-07-12 | 摘要初稿：14 字段 schema、TargetCatalog、scope-local context、sink 降级及 Audit 分离 | #793 |
| 2026-07-15 | 增加 `aemeath:llm-api-error` 独立诊断 sink、受控 JSON payload 与 Provider-owned 脱敏边界 | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-16 | 冻结 Logging Target 物理目录：`schema`/`filter`/`routing`/`sink`/`lifecycle` 技术管线；明确不建 `capabilities/`（各目录是同一诊断管线阶段，无独立业务状态所有权） | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
| 2026-07-15 | §10 Target 目录从扁平管线改为 Hexagonal（`domain + ports + adapters`），对齐 #972 v2 修订 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
