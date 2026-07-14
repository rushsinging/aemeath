# Provider · 端口、流与 Invocation Scope

> 层级：02-modules / provider（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#788（S2）
> 本文定义 ProviderPort 调用契约、流式与取消语义、Runtime/Provider 重试边界，以及不可变 Transport + Invocation Scope 生命周期。签名用于表达职责，不锁定具体 Rust API。

## 1. ProviderPort

`ProviderPort` 是 Agent Runtime 拥有的出站端口，Provider adapter 实现：

```rust
trait ProviderPort: Send + Sync {
    fn capabilities(&self, model: &ModelId)
        -> Result<ModelCapability, ProviderError>;

    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &dyn CancellationSignal,
    ) -> Result<InvocationStream, ProviderError>;
}

struct InvocationStream {
    events: ProviderEventStream,
}

enum InvocationEvent {
    Delta(InvocationDelta),
    Completed(ProviderCompletion),
    Failed(ProviderError),
}

impl InvocationStream {
    async fn next(&mut self) -> Option<InvocationEvent>;
}
```

核心语义：

- `invoke` 建立一次上游调用并返回有序流；
- `Delta` 只表达非终结增量；
- `Completed` 与 `Failed` 是互斥终结事件，恰好出现一个；取消以 `Failed(Cancelled)` 终结；
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

## 3. 流生命周期

```text
Created
  └─ invoke ─▶ Streaming
                  ├─ delta* ─▶ Streaming
                  ├─ completed event ─▶ Completed(response)
                  ├─ failed event ────▶ Failed(error)
                  └─ cancelled error ─▶ Cancelled
```

这是单次协议流的局部生命周期，不是 Agent 执行状态机。它不得驱动、复制或持久化 Run 状态。

### 3.1 顺序与终结

- delta 按 wire 顺序输出；
- 同一流的 decoder 不并发投递导致重排；
- `ProviderCompletion.output` 必须是所有已发 delta 的完整最终形态，并保留 provider tool-call ID；
- Runtime 消费 delta/completion 时创建领域 ToolCallId、组装自己的 `InvocationResponse.message` 与 Run Step ToolCall；
- `ProviderCompletion` 包含完整 output、stop reason、final usage snapshot 与 effective reasoning；
- 完成、失败、取消互斥且恰有一个；
- consumer drop 等价于取消意图，adapter 应停止继续读取和缓冲。

### 3.2 可见内容与重复调用

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

```rust
struct InvocationScope {
    transport: Arc<ProviderTransport>,
    model: ModelId,
    capability: ModelCapability,
    max_output_tokens: OutputTokenLimit,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
}
```

Invocation Scope 是一次调用尝试的不可变配置：

- 由 InvocationRequest + capability resolver 构造；
- 构造时完成 model 校验、output limit clamp 和 reasoning clamp；
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

Composition Root 唯一负责：

- 从 ConfigSnapshot 解析 provider、endpoint、认证、proxy 与 driver；
- 构造和缓存不可变 ProviderTransport；
- 构造 capability resolver 与 model overrides；
- 把 ProviderPort adapter 注入 RuntimeContext；
- 确保 Main/Sub 只共享无调用期可变状态的对象；
- 在配置快照变化时构造新 transport/factory generation，而不是原地改写正在调用的 scope。

```rust
trait ProviderFactory {
    fn for_model(&self, model: &ModelId)
        -> Result<Arc<dyn ProviderPort>, ProviderError>;
}
```

Factory 是组合根接线概念，不进入 Runtime 领域模型。RuntimeContext 只看到已经解析完成的 `Arc<dyn ProviderPort>`。

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

## 12. 架构守卫目标

#763 落地时应加入并故意违规验证以下规则：

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

## 13. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型与 ACL：[01-domain-model-and-acl.md](01-domain-model-and-acl.md)
- Runtime 模块边界：[../runtime/02-module-boundaries.md](../runtime/02-module-boundaries.md)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：ProviderPort、流/取消、Runtime 重试边界、不可变 Transport 与 Invocation Scope | #788 |
