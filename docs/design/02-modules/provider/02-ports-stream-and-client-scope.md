# Provider · 端口、流与 Invocation Scope

> 层级：02-modules / provider（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#788（S2）
> 本文定义 ProviderPort 调用契约、流式与取消语义、Runtime/Provider 重试边界，以及不可变 Transport + Invocation Scope 生命周期。签名用于表达职责，不锁定具体 Rust API。

## 1. ProviderPort

`ProviderPort` 是 Agent Runtime 拥有的出站端口（Runtime-owned），Provider adapter 只负责实现该签名。唯一权威 trait 定义在 [Runtime 端口与装配 §2](../runtime/06-ports-and-adapters.md#2-runtime-消费的能力契约)；本文 **NEVER** 复制第二份 trait 真相，只展开 Provider 侧调用语义、流事件类型与关联 Published Language：

```rust
// 权威签名见 ../runtime/06-ports-and-adapters.md §2（trait ProviderPort），
// 本文不重复定义：capabilities / resolve_invocation_options / invoke 三个方法。

enum InvocationEvent {
    Delta(InvocationDelta),
    Completed(ProviderCompletion),
    Failed(ProviderError), // 包含 ProviderErrorKind::Cancelled
}

// 具体实现可使用 `Stream<Item = InvocationEvent>` 的关联类型或装箱流；
// channel / SSE frame / reqwest bytes 均保持 adapter 私有。
```

核心语义：

- `resolve_invocation_options` 是 model capability clamp 的唯一入口；返回值供同一次 Context build 与 invoke 共同使用；
- `invoke` 建立一次上游调用并返回有序流；
- `Delta` 只表达非终结增量；
- `Completed` 与 `Failed` 是互斥终结事件，恰好出现一个；取消统一表示为 `Failed(ProviderErrorKind::Cancelled)`，不发布第二条取消终结通道；
- 终结事件后下一次 `next` 必须返回 `None`，不再提供独立 `finish()` 造成二次终结；
- consumer 可在本地根据全部 Delta + Completed 组装 attempt 结果；
- 具体 Rust 实现可使用关联 Stream 类型，但 Published Language 不暴露 Tokio channel、reqwest bytes、SSE frame 或 callback handler。

## 2. 为什么不用多方法回调

Provider 不发布 `on_text/on_thinking/on_tool_use/on_error` 一组回调，原因是：

1. 新增语义需要扩 trait，所有消费者同时破坏；
2. 字符串 `on_error` 丢失稳定分类；
3. `&mut handler` 把流读取与消费者生命周期紧耦合；
4. callback 难以表达背压、取消与唯一终结；
5. Runtime 需要把 delta 组装成 ModelInvocation，并投影领域事件，而不是让 Provider 直接调用 UI handler。

目标流保持 pull-based 自然背压。若适配器内部使用 channel，必须有界并在消费方取消/丢弃时停止生产，channel 类型仍保持私有。

> **当前落地（#903/#907）**：Provider 生产入口 `LlmProvider::invocation_stream` 返回 pull-based `InvocationStream`，Runtime Main/Sub/Reflection/Compact 主动 poll `InvocationEvent` 并经 reducer 组装结果。#907 已物理清零旧 `LegacyStreamSink` / gateway callback wrapper / client pool 与 `set_*→调用→restore` 路径——这些符号已从代码库删除。Provider driver 内部保留的 `InvocationSink`（`pub(crate)` trait）是私有 decoder seam：它把各 driver 的供应商 SSE/line event 归一到 `InvocationDelta`，是 adapter 内部实现细节而非 legacy 迁移桥，**NEVER** 经跨 BC 暴露也不得误称为 legacy sink。Runtime 与 Context 的生产代码和测试替身均被架构守卫禁止引用 Provider 内部 `InvocationSink` 或任何 legacy callback symbol。

## 3. 流生命周期

```text
Created
  └─ invoke ─▶ Streaming
                  ├─ delta* ─▶ Streaming
                  ├─ completed event ─▶ Completed(response)
                  ├─ failed event ────▶ Failed(error)
                  └─ cancelled error ─▶ Failed(ProviderErrorKind::Cancelled)
```

这是单次协议流的局部生命周期，不是 Agent 执行状态机。它不得驱动、复制或持久化 Run 状态。

### 3.1 顺序与终结

- delta 按 wire 顺序输出；
- 同一流的 decoder 不并发投递导致重排；
- `ProviderCompletion.output` 必须是所有已发 delta 的完整最终形态，并保留 provider tool-call ID；
- Runtime 消费 delta/completion 时创建领域 ToolCallId、组装自己的 `InvocationResponse.message` 与 Run Step ToolCall；
- `ProviderCompletion` 包含完整 output、stop reason、final usage snapshot 与 effective reasoning；
- 完成与失败互斥且恰有一个；显式取消、consumer drop、背压与 producer 异常都统一为 `Failed(ProviderError)`，取消使用 `ProviderErrorKind::Cancelled`；
- 首个终态后必须结束，禁止再产出任何事件；
- consumer drop 等价于取消意图，adapter 应停止继续读取和缓冲；若使用 channel，producer 必须持有 bounded send permit，或使用不依赖 receiver 存活的独立终态状态，保证失败可观察，禁止仅依赖向已关闭 channel 发送终态；
- `ProviderToolCallStart.id` 是 invocation-scoped 内置 `ProviderToolCallId`；adapter 必须先按稳定 stream index 建立该 ID，待 provider id 到达后再绑定，禁止直接把 provider id 当作领域身份。

### 3.2 可见内容与重复调用

> **当前落地（#1033/#905）**：Provider 已用 crate-private `HttpAttemptExecutor`（`adapters/http_attempt.rs`）统一单 attempt 的机械发送、cancellation、status/error body 判定与单一 diagnostic 记录，driver 之间不再各自复制这段逻辑。#905 已关闭：跨调用 retry/backoff（P6）与 stream→non-stream fallback（P7）的所有权已迁至 Runtime，错误分类统一（P9）已收口。Provider 生产 stream adapter 被 `check-provider-retry-ownership.sh` 守卫禁止恢复 retry loop、backoff sleep 或 fallback 发起逻辑。

ProviderPort 的一次 invoke 只允许一次上游语义请求。Provider 不实现“stream 失败后自动 non-stream 重发”，因为第二次请求可能产生不同文本、工具调用或副作用意图，也会让 Runtime 无法准确记录 attempt 和 usage。

任何第二次调用（普通 retry 或 non-stream fallback）都必须由 Runtime 显式开始新的 attempt，并满足：

- 前一次尚未向 EventSink 提交可见 delta；或 Runtime 具备明确、原子的 attempt rollback 后再重试；
- 若可见 delta 已提交且无法回滚，流中断即使标记 retryable，也不得自动重试，必须保留部分输出并按失败策略终结；
- 新 attempt 有独立编号、事件和 usage；
- 取消优先于 retry/fallback；
- 策略可观察、可测试，不藏在 driver 内部。

## 4. Cancellation

```rust
trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
}
```

该 Published Language 表达只读取消意图，不绑定 Tokio。Runtime 创建 cancellation tree 并适配到端口，Provider 必须在以下等待点响应：

1. 请求编码前；
2. 获取连接和发送 HTTP 请求；
3. 等待 response headers；
4. 每次读取 stream frame；
5. decoder 等待或内部有界队列阻塞；
6. 收割最终响应。

取消返回 `ProviderErrorKind::Cancelled`，不标记 retryable，不伪装成 timeout/network。Provider 不拥有 timeout 时长；Runtime 的 deadline 到期后触发 cancellation，并决定 Run/attempt 语义。

## 5. Runtime 与 Provider 的重试分工

### 5.1 Provider

Provider 负责：

- 单次请求的协议正确发送；
- 状态码、错误 body、网络错误和流截断分类；
- 提供 `retryable` 与安全 provider code；
- 尊重 `Retry-After` 等协议 hint，并作为错误元数据返回；
- 保证失败 attempt 的 delta、usage 和终结语义可归因。

> **当前落地（#1033/#905）**：以上机械已收敛进 crate-private `HttpAttemptExecutor`——单 attempt 的 cancellation-aware send/status 判定、安全 response headers 提取、16KiB 有界 error body 读取、typed network/HTTP transport failure 分类，以及唯一一条 `llm-api-error` diagnostic；Anthropic、OpenAI-compatible 与 Ollama 已全部迁入。#905 已关闭：错误分类统一（P9）、跨调用 retry/backoff（P6）与 stream→non-stream fallback（P7）的所有权均已迁至 Runtime。

Provider 不负责：

- 自动开始第二个模型请求；
- attempt 计数和指数退避；
- context 超限后的 compact；
- 切换 model/provider；
- 决定 Run 最终失败。

### 5.2 Runtime model_invocation

Runtime 负责：

1. 创建 attempt 与 cancellation scope；
2. 发出 `ModelInvocationStarted`；
3. 调 ProviderPort 并把 delta 应用到本 attempt；
4. 跟踪本 attempt 的 delta 是否已提交到 EventSink；只有未提交或已原子回滚时，才根据 ProviderErrorKind、retryable、retry_after 决定重试；
5. 退避期间保持可取消；
6. 发出 `ModelInvocationRetrying{attempt}`；
7. ContextTooLong 时调用 Context Management compact 后构建新请求；
8. 成功后组装 ModelInvocation VO；
9. 达到上限或 fatal 时推动 Run 失败。

统一目标策略是 Retryable 指数退避、最多 10 次、退避封顶 5 分钟；该策略属于 Runtime，不写入 Provider adapter。

## 6. 不可变 Transport

```rust
struct ProviderTransport {
    provider: ProviderId,
    endpoint: Endpoint,
    auth: AuthHandle,
    http: HttpTransport,
    driver: Arc<dyn ProviderDriver>,
}
```

Transport 在构造后不可变，生命周期可覆盖多个 Run：

- HTTP connection pool 可跨 Main/Sub 共享；
- endpoint、认证 handle、TLS、proxy 和 driver identity 固定；
- 认证刷新若需要内部同步，只能改变凭证机制的内部缓存，不得改变调用 model/options；
- Transport 不保存 current model、current reasoning、current max tokens 或 current handler；
- pool 只缓存不可变 Transport，不缓存可被调用方改写的 invocation 配置。

Transport pool 的 key 至少能唯一标识 provider endpoint、认证域和 driver；model 只有在它确实决定 transport 时才进入 key。找不到指定 provider/model 时显式报 Configuration/ModelUnavailable，不静默回退默认 client。

## 7. Invocation Scope

> **当前落地（#902/#903/#907）**：`provider::InvocationScope` 已冻结 `model / max_tokens / requested_reasoning / effective_reasoning`，并显式传入 `LlmProvider::invocation_stream`。Anthropic、OpenAI-compatible 与 Ollama 请求构造只读该 scope；provider atomics / setter、Runtime shared-client lock 与 finalize restore 已物理删除（`check-provider-invocation-scope.sh` 守卫阻止回流）。下方 `transport / capability_fingerprint / OutputTokenLimit` 仍是后续完整 ProviderPort 切线的 Target，不应误读为当前 Rust 类型已全部具备。

```rust
struct InvocationScope {
    transport: Arc<ProviderTransport>,
    model: ModelId,
    capability_fingerprint: CapabilityFingerprint,
    max_output_tokens: OutputTokenLimit,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
}
```

Invocation Scope 是一次调用尝试的不可变配置：

- 由 `InvocationRequest.options` 直接构造；
- model 校验、output limit clamp 和 reasoning clamp 已在 Runtime 构建 Context 前由 `resolve_invocation_options` 完成；
- `invoke` 在编码前复核 capability fingerprint；若失效则返回 `CapabilityChanged`，**NEVER** 静默重算 effective reasoning；
- 建立后不提供 `set_model/set_max_tokens/set_reasoning_level`；
- driver 编码只读取 scope；
- attempt 结束即释放，不需要 restore；
- 新 attempt 构造新 scope，防止重试期间配置漂移。

若一个 Run 的多次调用共享相同 defaults，可共享不可变 `InvocationDefaults`，但每次 attempt 仍需生成自己的 scope，不能把 defaults 当成可变 current state。

## 8. Main/Sub 隔离

Main 与每个 Sub Run 都可共享同一个 `Arc<ProviderTransport>`，但不得共享可变 Invocation Scope：

```text
                       Arc<ProviderTransport>
                         /        |        \
                        /         |         \
             Main Invocation  Sub A Invocation  Sub B Invocation
             model=M1          model=M2           model=M3
             effort=High       effort=Low         effort=Max
             max=32k           max=8k             max=16k
```

并发安全来自：

1. Transport 只读共享；
2. 每个 invocation 配置按值不可变；
3. 流 decoder 状态属于单次 invocation；
4. 没有跨调用 `set_*`；
5. 没有 finalize restore；
6. panic、cancel 或提前 drop 不会污染其他调用。

因此 Main/Sub 的隔离语义是**独立 invocation state**，而非复制 socket pool。目标装配可以返回同一 ProviderPort 实现，只要实现满足上述状态隔离不变量。

## 9. Composition Root 与 Factory

> **当前落地（#907）**：Composition Root 已通过 Runtime-owned `ProviderFactory` trait（`runtime::ports::provider_factory`）独占 provider 构造。`ProviderFactory::build(spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError>` 接收纯值 spec 并返回 `ProviderBinding`（`Arc<dyn ProviderPort>` + model / max_tokens / reasoning / context_window）。Provider crate 的 `provider::composition` 模块是 Composition Root 专用构造面，重新导出 `LlmClient` / `LlmConfigOptions` / `InvocationScope` / `SystemBlock` / `LlmProvider` 等具体构造符号；非 Composition crate **NEVER** 引用 `provider::composition` 或构造符号。`check-provider-construction-ownership.sh` 守卫以零白名单锁定此边界并含负向探针证据。

Composition Root 唯一负责：

- 从 ConfigSnapshot 解析 provider、endpoint、认证、proxy 与 driver；
- 构造和缓存不可变 ProviderTransport；
- 构造 capability resolver 与 model overrides；
- 实现 `ProviderFactory`，把 `ProviderBuildSpec` 解析为 `ProviderBinding`；
- 把 ProviderPort adapter 注入 RuntimeContext；
- 确保 Main/Sub 只共享无调用期可变状态的对象；
- 在配置快照变化时构造新 transport/factory generation，而不是原地改写正在调用的 scope。

```rust
/// Runtime-owned factory trait（定义在 runtime::ports）。
trait ProviderFactory: Send + Sync {
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError>;
}

/// Composition Root 生产构造结果。
struct ProviderBinding {
    provider: Arc<dyn ProviderPort>,
    model: ModelId,
    max_tokens: u32,
    requested_reasoning: ReasoningLevel,
    context_window: Option<usize>,
}
```

Factory 是组合根接线概念，不进入 Runtime 领域模型。RuntimeContext 只看到已经解析完成的 `ProviderBinding` / `Arc<dyn ProviderPort>`。Runtime Main/Sub/Reflection/Compact 均只依赖 `ProviderFactory` / `ProviderBinding` / `ProviderPort` 与 Provider Published Language。

## 10. 并发与资源限制

- `ProviderPort` 必须 `Send + Sync`；
- 单个 `InvocationStream` 只允许一个消费者，保证顺序；
- 全局/供应商并发限制可由 transport adapter 的资源机制执行，但业务优先级与 Run 调度归 Runtime；
- 限流等待必须可取消；
- buffer 必须有界，禁止无限积压 token delta；
- decoder 不持有 RuntimeContext、EventSink 或 TUI sender；
- 多个 Sub Run 并行调用时，任何一个 scope 的失败、取消或 drop 不影响其他 scope。

## 11. 可观测性与安全

每次 attempt 的日志应能关联：run_id、run_step_id、invocation/attempt id、provider、model、driver、effective reasoning、HTTP status、duration、usage 和错误类别。关联 ID 由 Runtime 传入诊断上下文或在 adapter 边界附加，不让 Provider 持有 Run 聚合。

禁止记录：

- API key、Authorization header、完整环境变量；
- 未脱敏的请求 body 与用户敏感上下文；
- 未清洗的供应商错误 body；
- 完整图片/base64 内容；
- tool arguments 中的潜在 secret，除非经过统一审计策略。

Provider 诊断日志归 Logging；原始 usage 经 Runtime 发布给 Audit。二者不能混成一个 sink。

Provider API 失败统一写注册 target `aemeath:llm-api-error`（`llm-api-error.log`），使用 14 字段 DiagnosticRecord 的 `msg` 承载受控 JSON payload。payload 由 Provider adapter 在错误事实最完整的边界构造；Logging 不解析 vendor body。应尽可能包含 driver/API、provider/model、调用关联、已清洗 endpoint、method/status、provider request ID、typed error kind/code、retryable、attempt/max attempts、retry-after、elapsed、请求/响应计数与字节统计、partial-output、截断脱敏 preview 和 source chain。取消不写错误日志；中间可重试失败为 debug，最终失败为 error；同一 attempt 同一失败只写一条。详细安全边界见 [Logging 设计](../logging/README.md#51-llm-api-error-payload-与脱敏边界)。

## 12. 架构守卫目标

#982 落地时 **MUST** 加入并故意违规验证以下规则，由 #763 汇总验收：

```text
Rule: provider-wire-types-stay-inside-adapters
Allow: Provider adapter/driver modules
Deny: Agent Runtime, Context Management, SDK, TUI imports of wire DTO

Rule: provider-construction-owned-by-composition
Allow: agent/composition/**
Deny: Runtime/application modules constructing concrete providers/transports

Rule: provider-invocation-state-is-immutable
Deny: production set_model/set_max_tokens/set_reasoning_level on shared Provider/client
```

守卫应优先检查 AST/path 与公开 re-export，不依赖简单文件名黑名单。新增白名单必须记录 owner、理由和退出条件。

> **已落地（#1033/#907）**：`check-provider-http-attempt.sh` 已启用（§6c），锁定"driver 只能经 `HttpAttemptExecutor::execute` 发送请求、只能经其 `BoundedErrorBody` 读取失败响应体、HTTP/network 诊断日志 API 仅限 `http_attempt.rs` + `error_log.rs` 调用"三条不变量。`check-provider-construction-ownership.sh`（§6g）以零白名单锁定 #907 构造所有权：非 Composition crate 禁止引用 `provider::composition` 或具体构造符号（`LlmClient` / `LlmConfigOptions` / `InvocationScope` / `SystemBlock` / `LlmProvider`），正向断言 `provider::composition` 至少被 Composition 生产代码引用；负向探针（在非 Composition 源文件中追加 `provider::composition::LlmClient` 引用）以 exit 2 命中，移除后 clean pass。详见 [Architecture Guards §6c/§6g](../../03-engineering/01-architecture-guards.md)。

## 13. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型与 ACL：[01-domain-model-and-acl.md](01-domain-model-and-acl.md)
- Runtime 模块边界：[../runtime/02-module-boundaries.md](../runtime/02-module-boundaries.md)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-19 | #907 完成 Adapter 最终收口：Runtime Main/Sub/Reflection/Compact 只依赖 `ProviderFactory` / `ProviderBinding` / `ProviderPort` 与 PL；`provider::composition` 独占构造（`check-provider-construction-ownership.sh` 零白名单 + 负向探针）；旧 `LegacyStreamSink` / gateway callback / pool / setter restore 物理清零；Provider 内部 `InvocationSink`（`pub(crate)`）为私有 decoder seam 非 legacy；#905 已关闭 P6/P7/P9；#1142 resolver 接线仍延期 | [#907](https://github.com/rushsinging/aemeath/issues/907) |
| 2026-07-17 | #903 将 Provider→Runtime 生产链切换为 pull-based `InvocationStream`：`Completed/Failed` 单终结、取消统一为 `Failed(Cancelled)`、Runtime/Context 主动 poll 且禁止跨 crate legacy sink；Provider decoder 内部迁移桥作为明确残余登记 | [#903](https://github.com/rushsinging/aemeath/issues/903) |
| 2026-07-16 | #1033 交付 crate-private `HttpAttemptExecutor`：收敛单 attempt 机械 send/cancel/status、安全 headers、16KiB bounded error body、typed transport failure 分类与单一 diagnostic，并新增 `check-provider-http-attempt.sh` 守卫；跨调用 retry/fallback（P6/P7）仍是 Runtime 待迁移债，本次改动不冒充其已完成 | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-16 | 文档审查：明确后续承接边界——pull-based `InvocationStream`（P4）由 [#903](https://github.com/rushsinging/aemeath/issues/903) 承接；跨调用 retry/backoff（P6）、stream→non-stream fallback（P7）与错误分类统一（P9）由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接 | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-12 | 初稿：ProviderPort、流/取消、Runtime 重试边界、不可变 Transport 与 Invocation Scope | #788 |
| 2026-07-14 | 增加 build 前 option resolution；Context prompt 与 InvocationScope 共享唯一 effective reasoning / limits 快照 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | `ProviderPort` 明确 Runtime-owned；移除本文重复的 trait 定义，改为引用 [Runtime 06 §2](../runtime/06-ports-and-adapters.md#2-runtime-消费的能力契约) 的唯一签名 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | Provider API 失败统一进入 `aemeath:llm-api-error` 独立 sink；冻结 attempt 级关联、脱敏 JSON payload、取消排除和单失败单记录规则 | [#700](https://github.com/rushsinging/aemeath/issues/700) |
