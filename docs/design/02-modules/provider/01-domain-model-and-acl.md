# Provider · 领域模型与 ACL

> 层级：02-modules / provider（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#788（S2）
> 本文只描述目标态；签名用于表达职责和不变量，不锁定具体 Rust API。实现差距统一记录在 `03-engineering/migration-governance.md`。

## 1. 边界语言

`ProviderPort` 定义在 Agent Runtime 一侧，因此调用请求、delta 与错误首先服务 Runtime 的 `Model Invocation` 语义。Provider adapter 实现该语言，不把供应商协议类型提升为领域模型。

```rust
struct InvocationRequest {
    model: ModelId,
    window: ContextWindow,
    options: ResolvedInvocationOptions,
}

struct RequestedInvocationOptions {
    requested_max_output_tokens: Option<OutputTokenLimit>,
    reasoning: ReasoningLevel, // Workflow 已应用 Config user maximum
}

struct ResolvedInvocationOptions {
    context_size: TokenCount,
    max_output_tokens: OutputTokenLimit,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
    capability_fingerprint: CapabilityFingerprint,
}

struct ProviderCompletion {
    output: ProviderAssistantOutput,
    stop_reason: StopReason,
    usage: Option<RawUsageSnapshot>,
    effective_reasoning: ReasoningLevel,
}

struct ProviderAssistantOutput {
    content: Vec<ProviderContentBlock>,
    tool_calls: Vec<ProviderToolCall>,
}
```

边界对象只包含稳定值：领域 Message、system blocks 与模型可见 Tool schema 组成的 Context Window 发布视图、已解析调用选项和 Provider Completion。`ContextWindow.tool_schemas` 是 Tool Catalog 单次快照经 Context Management 原样带入的唯一 schema 集；`InvocationRequest` **NEVER** 再复制第二个 `tools` 字段。最终 output 在 Provider 边界保留 `ProviderToolCallId`；Runtime 消费 delta/completion 时创建领域 `ToolCallId`、维护双 ID 映射，并组装自己的 `InvocationResponse.message` 与 Run Step ToolCall。Provider 不自行生成领域 ID。这些对象不得携带 Session、Run、RuntimeContext、HTTP client、driver、provider config 或回调 handler。

### 1.1 所有权

| 类型 | 所有者 | Provider 的角色 |
|---|---|---|
| `ContextWindow` / `Message` | Context Management / Shared Kernel | 读取并转换，不改变历史语义 |
| `InvocationRequest/Delta/Event/Error` | Agent Runtime 出站端口 PL | 实现并返回 |
| `ProviderCompletion` / `ModelCapability` | Provider PL | 声明、解析并返回 |
| 供应商 request/response/event DTO | Provider adapter 私有 | 独占 |
| `ModelInvocation` | Agent Runtime | 不创建、不持久化 |
| `RawUsageSnapshot` | Provider 提取，Audit 消费 | 只做协议标准化 |

## 2. 模型能力

Provider 必须显式声明目标模型的调用能力，避免 Runtime 依赖 provider 名称、模型前缀或散点特判。

```rust
struct ModelCapability {
    model: ModelId,
    modalities: ModelModalities,
    supports_tools: bool,
    supports_parallel_tool_calls: bool,
    supports_streaming: bool,
    reasoning: ReasoningCapability,
    context_limit: Option<TokenCount>,
    output_limit: Option<TokenCount>,
}

struct ReasoningCapability {
    supported: ReasoningLevels,
    maximum: ReasoningLevel,
    mapping: ReasoningMappingKind,
}

enum ReasoningMappingKind {
    Effort,
    ThinkingToggle,
    ThinkingBudget,
    Adaptive,
    None,
}
```

能力解析顺序：

1. driver 提供协议族默认能力；
2. model capability table 按稳定 model identity 覆盖；
3. ConfigSnapshot 提供部署级显式覆盖；
4. 无法确认的能力采用保守值，不能假定最高档位或高级特性可用。

`ModelCapability` 是只读 Published Language。Runtime 可用于前置校验和展示，但 Provider 在请求编码前仍必须复核，防止陈旧快照或绕过调用。

## 3. Reasoning 两阶段 clamp 与冻结

统一职责链：

```text
Requested Reasoning = min(Workflow Desired, Config User Maximum)
Effective Reasoning = greatest supported level <= Requested Reasoning
```

其中：

- Config 拥有用户默认值、静态上限及来源优先级；
- Workflow 消费 ConfigSnapshot，把动态 desired 裁剪为 requested reasoning；
- Runtime 在构建 Context Window **之前**调用 Provider-owned `resolve_invocation_options`；
- resolver 根据 driver + model 的 `supported` 档位集合选择不高于 requested 的最高档位，同时解析 context/output limits，并返回不可变 `ResolvedInvocationOptions`；
- Runtime 把同一个 `effective_reasoning` 同时放入 `ContextRequest`（供 Prompt guidance）与 `InvocationRequest.options`；
- Provider `invoke` 只校验 `capability_fingerprint` 后映射 wire 字段，**NEVER** 静默再次 clamp。若 capability 已变化则返回 `CapabilityChanged`，Runtime 丢弃旧 window，重新 resolve + build；
- Provider 在最终响应中回报 effective level，便于审计和诊断。

因此单次模型调用只有一条 reasoning 数据流：

```text
ReasoningPort.current_requested_level
  → ProviderPort.resolve_invocation_options
  → ResolvedInvocationOptions.effective_reasoning
       ├─ ContextRequest.effective_reasoning → PromptRequest.effective_reasoning
       └─ InvocationRequest.options.effective_reasoning → InvocationScope
```

### 3.1 映射规则

ReasoningLevel 是跨 BC 的统一抽象：`Off / Low / Medium / High / Xhigh / Max`。driver 必须把 effective level 映射为本协议支持的形式，例如 effort 字符串、thinking 开关、token budget 或 adaptive mode。

- 不支持的档位必须在有序 `supported` 集合中向下选择，禁止向上扩大；
- `Off` 不得被默认开启逻辑反向提升；
- 仅支持开关的模型必须把可表达集合声明为 `{Off, OnLevel}`；任意非 Off 请求映射到不高于请求的 `OnLevel`，effective level 报告该规范化档位；
- 映射逻辑由 driver 策略封装，Runtime 不出现供应商字段名；
- capability 未知时采用保守 supported 集合，并生成结构化诊断。

## 4. Driver 模型

Driver 是 Provider adapter 内部的协议策略，不是 BC 对外端口：

```rust
trait ProviderDriver {
    fn capabilities(&self, model: &ModelId) -> ModelCapability;
    fn encode_request(&self, request: &EffectiveInvocation) -> WireRequest;
    fn decode_stream(&self, input: WireStream) -> DriverEventStream;
    fn decode_error(&self, status: HttpStatus, body: SanitizedBody) -> ProviderError;
}
```

`WireRequest`、`WireStream`、`DriverEvent` 均为 Provider 内部类型。Anthropic、OpenAI-compatible 与 Ollama 可以复用公共 codec 或工具函数，但不能靠“兜底伪装成另一协议”掩盖不支持路径；不适用的 driver 组合必须在装配或能力解析阶段失败。

OpenAI-compatible 仍允许各部署策略覆盖：max token 字段、reasoning 字段、tool-call 细节、usage 位置和 stop reason；这些差异应通过小型策略对象组合，而不是复制完整 Provider 实现。

## 5. 入站 ACL：领域 → wire

请求转换必须显式构建供应商 wire format，禁止对领域 Message 直接 `serde_json::to_value` 后透传。

### 5.1 Message

ACL 对每个 ContentBlock 显式映射：

- Text → 对应协议文本块；
- Image → 供应商支持的 base64/data URL/native images 格式；
- ToolUse → 供应商 function/tool-call 结构，并保留 provider 边界 ID；
- ToolResult → 对应 tool result/message，修复协议要求的关联顺序；
- Thinking → 仅在协议要求且签名/字段完整时回传；否则安全剥离；
- metadata、placeholder、内部 text cache 等非 wire 字段一律不得泄漏。

### 5.2 Tool schema

`ModelToolSchema` 是 Tool Catalog 的模型可见投影。Runtime 每次 PreparingContext 只拉取一个 `ToolCatalogSnapshot`，按 snapshot 稳定顺序生成 schema 集并放入 `ContextRequest`；Context Management 在预算后把同一集合原样放入 `ContextWindow.tool_schemas`。driver 转换时只保留供应商允许字段：名称、描述、输入 schema 及明确支持的 cache hint；内部 capability、resource、data schema、函数引用与 Registry 信息不得出站。Context 与 Provider **NEVER** 再查询 Catalog 或重新排序。

### 5.3 System 与缓存提示

Provider 只执行协议级表达：system message/block、cache-control 断点等。提示内容和片段顺序由 Context Management 决定；Provider 不重新排序业务 prompt，也不决定哪些内容应该进入 Context Window。

## 6. 出站 ACL：wire → 统一语义

流 decoder 把供应商事件转换为有序 `InvocationDelta`：

```rust
enum InvocationDelta {
    Text(TextDelta),
    Thinking(ThinkingDelta),
    ToolCallStarted(ProviderToolCallStart),
    ToolArgumentsDelta(ProviderToolArgumentsDelta),
    ToolCallCompleted(ProviderToolCall),
    UsageSnapshot(RawUsageSnapshot),
}
```

Provider 边界中的 tool-call 标识保持 `ProviderToolCallId`，Runtime 在写入 Run Step 时创建领域 `ToolCallId` 并维护双 ID 映射。Provider 不生成领域 ToolCallId。

### 6.1 Tool arguments

- delta 必须按供应商顺序关联到稳定的 provider call ID 或 stream index；
- arguments 可增量输出字符串片段，但 complete 事件必须给出验证过的 JSON 值；
- 流结束时若 arguments 不完整，返回结构化 `StreamTruncated`，不得伪造空对象；
- complete 后不得继续为同一 tool call 发参数 delta。

### 6.2 StopReason

供应商 stop reason 统一为封闭语义：

```rust
enum StopReason {
    EndTurn,
    ToolUse,
    MaxOutputTokens,
    ContentFiltered,
    StopSequence,
    Other(ProviderStopCode),
}
```

未知代码必须保留安全、有限的 provider code 供诊断，不能丢失为通用字符串，也不能影响 Runtime 的穷尽处理安全性。

## 7. Raw Usage

```rust
struct RawUsageSnapshot {
    input_tokens: Option<TokenCount>,
    output_tokens: Option<TokenCount>,
    cache_read_tokens: Option<TokenCount>,
    cache_write_tokens: Option<TokenCount>,
    reasoning_tokens: Option<TokenCount>,
}
```

规则：

1. 所有字段都区分“未报告”与真实零值；供应商完全不返回 usage 时，最终响应的 usage 为 `None`；
2. `UsageSnapshot` 一律表示当前 attempt 的累计快照，不发布增量计数；driver 必须在 ACL 内把供应商的累计/增量 wire 语义转换为累计值；
3. 新快照不得让已知计数倒退；重复快照按覆盖而非相加处理；
4. 最终响应至多有一个 final usage snapshot，并与最后一次 delta 快照一致；
5. Provider 不读取价格表、不计算货币成本；
6. Runtime 负责把 snapshot 关联到 attempt/Model Invocation，Audit 负责定价和聚合。

## 8. 错误分类

```rust
enum ProviderErrorKind {
    Cancelled,
    Authentication,
    PermissionDenied,
    RateLimited,
    ContextTooLong,
    InvalidRequest,
    ModelUnavailable,
    UpstreamUnavailable,
    Network,
    Timeout,
    Protocol,
    StreamTruncated,
    Configuration,
}

struct ProviderError {
    kind: ProviderErrorKind,
    retryable: bool,
    safe_message: String,
    provider_code: Option<ProviderErrorCode>,
    retry_after: Option<Duration>,
}
```

- `retryable` 是 Provider 对失败性质的提示，不是重试命令；
- `retry_after` 只承载经校验的协议等待 hint，Runtime 决定是否采用并施加自身上限；
- HTTP 429、部分 5xx、瞬时网络与流中断通常可标记 retryable，但 driver 可按协议细化；
- 认证、权限、非法请求通常为 fatal；
- context 超限必须独立分类，让 Runtime 选择 compact；
- 取消必须独立分类，不得包装成 Network/Stream；
- `safe_message` 不得包含 API key、认证 header、完整请求 body 或未经清洗的供应商响应；
- 原始错误可进入受控诊断日志，但必须先清洗。

## 9. 核心不变量

1. 一个 InvocationRequest 固定一个 model、一份不可变 resolved options 和一个已完整构建的 ContextWindow；
2. Provider 每次 invoke 最多执行一次上游语义请求；
3. delta 顺序与供应商流顺序一致，单流不并发重排；
4. 每个 provider tool call 的 start/arguments/complete 关联一致；
5. invoke 只能以一个 ProviderCompletion 或一个 ProviderError 终结；
6. 终结后不再输出 delta；
7. effective reasoning 是 Provider/model `supported` 集合中不高于 requested reasoning 的最高档位，且必须与同一请求的 ContextWindow prompt 所用值相同；
8. wire DTO 不得出现在 Provider adapter 之外；
9. RawUsageSnapshot 保留原始计量语义，不混入 cost；
10. Runtime 不需要依据 provider 名称解释任何响应。

## 10. 相关文档

- 模块入口：[README.md](README.md)
- 端口、流与 Invocation Scope：[02-ports-stream-and-client-scope.md](02-ports-stream-and-client-scope.md)
- Runtime Model Invocation：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- 依赖规则：[../../01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：调用边界语言、模型能力、三层 clamp、双向 ACL、usage 与错误分类 | #788 |
| 2026-07-14 | 在 Context build 前解析并冻结 model capability；InvocationRequest 直接复用 ContextWindow 的唯一 Tool schema 集 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
