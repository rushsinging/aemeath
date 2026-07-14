# Provider（通用域）

> 层级：02-modules / provider（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#788（S2）
> 本模块吸收各家 LLM API 的协议差异，对 Agent Runtime 提供统一模型调用能力。Provider 是通用域：它保证一次调用的协议正确性，但不拥有 Run 编排、跨调用策略或成本业务。

## 1. 模块定位

Provider 位于 Agent Runtime 与外部 LLM API 之间，是最重的防腐层（ACL）：

```text
Agent Runtime
    │ ProviderPort + Runtime-owned invocation language
    ▼
Provider adapter
    ├── capability resolution
    ├── reasoning clamp / wire mapping
    ├── request ACL
    ├── HTTP + stream decoder
    └── response/error/usage ACL
          │
          ▼
Anthropic / OpenAI-compatible / Ollama / ...
```

Provider 的价值不是统一所有供应商特性，而是把差异收敛到稳定语义：同一份领域 Message、Tool schema 和 reasoning 请求，经不同 driver 转成各自 wire format；响应再还原为统一 delta、最终响应、原始 usage 与结构化错误。

## 2. 核心决策

1. **Runtime 拥有出站端口**：`ProviderPort` 是 Agent Runtime 的出站端口，Provider 提供实现；端口签名不得由 HTTP client 或供应商 SDK 反向塑形。
2. **ACL 全部内聚于 Provider**：供应商 request/response/SSE DTO、header、URL、错误 body 和 tool-call wire 结构不得越过 adapter 边界。
3. **一次 invoke = 一次语义调用尝试**：Provider 不透明发起第二次模型调用；跨调用重试、退避、compact 后重试、降级与故障转移归 Runtime。
4. **流与终结态分离**：内容、thinking、tool arguments 和 usage 以有序 delta 输出；完成或结构化错误只能终结一次，终结后不得再发 delta。
5. **Reasoning 能力按 driver + model 解析**：Provider 声明模型能力并执行最终 clamp，再映射为供应商字段；Runtime 不理解 `reasoning_effort`、`thinking` 或 budget 等 wire 细节。
6. **原始 Usage，不计算 Cost**：Provider 提取供应商返回的 token usage；Audit 拥有 pricing、cost 与聚合。
7. **共享不可变传输，不共享调用期可变状态**：HTTP 连接池、endpoint 与 driver 可安全共享；model、max output tokens、reasoning 等必须固定在独立 Invocation Scope 中。
8. **Main/Sub 隔离是状态隔离**：Main/Sub 各自拥有独立调用配置和 Invocation Scope，不要求复制底层 HTTP client；任何 `set_* → 调用 → restore` 模式均不成立。
9. **Composition Root 唯一装配**：driver、凭证、endpoint、transport pool 与 ProviderPort adapter 只在组合根接线。

## 3. 责任边界

| Provider 负责 | Provider 不负责 |
|---|---|
| 供应商协议 ACL | Run / Run Step 状态机 |
| 单次调用的请求发送与流解析 | Context Window 构建与 compact 决策 |
| 模型能力声明与查询 | Workflow reasoning 阶段调节 |
| reasoning 能力 clamp 与 wire 映射 | 跨调用重试、退避、降级、故障转移 |
| stop reason、usage、错误分类 | Tool 执行、Policy、Hook、审批 |
| 取消传播到 HTTP 与流读取 | Pricing、Cost 聚合与审计策略 |
| wire 数据安全清洗与诊断 | SDK/TUI 事件投影 |

“Provider 不负责跨调用重试”不等于忽略网络机制。连接建立、协议解码和取消必须正确；一旦失败，Provider 应返回稳定错误类别和 retryable 提示，由 Runtime 决定是否开始下一次 invocation attempt。

## 4. 模块内部结构

```text
provider/
├── capability/                 # driver + model 能力解析、reasoning clamp
├── invocation/                 # 单次调用应用服务、统一流语义
├── transport/                  # 不可变 endpoint/auth/http transport
├── drivers/
│   ├── anthropic/              # request/stream/error ACL
│   ├── openai_compatible/      # 公共协议骨架 + 厂商策略
│   └── ollama/                 # 原生协议 ACL
├── error/                      # wire/HTTP 错误 → ProviderErrorKind
└── api/                        # ProviderPort adapter factory；不暴露 wire DTO
```

目录按业务能力表达，driver 是出站 adapter 的内部策略，不是对 Runtime 发布的对象。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 通过 `ProviderPort` 发起调用、消费有序流并组装 `ModelInvocation`。Runtime 拥有 attempt 编号、重试退避、`ModelInvocationRetrying` 事件、context 超限后的 compact 分支和最终 Run 失败判断。

### Context Management

Context Management 构建 Context Window；Provider 只接收已经装配完成的稳定调用输入，不读取 Session、Memory、Prompt 或 Guidance 的内部结构，也不自行 compact。

### Workflow / Config

Workflow 根据 Reasoning Node 与 Config 静态上限产生请求 effort。Provider 根据目标模型能力做最后一道 clamp，并把有效档位映射到 wire 字段。Provider 不读取散点 env，也不拥有用户默认值。

### Tool & Skill & Command

Provider 只接收 Tool Catalog 发布的模型可见 schema，并把模型输出转为 tool-call 语义；它不执行 Tool，不检查 Profile，不触发 Policy/Hook，也不路由 Slash Command。

### Audit

Provider 发布原始 usage；Runtime 将其关联到 Model Invocation 并发出审计事件。Audit 负责定价、成本计算、聚合与落盘策略。

## 6. 设计边界

- **NEVER** 向 Runtime 暴露具体 client、pool、driver、HTTP response、SSE event 或供应商 DTO。
- **NEVER** 在共享 Provider 实例上修改 model、max tokens、reasoning level 等调用期配置。
- **NEVER** 在已发出用户可见 delta 后透明重发模型请求。
- **NEVER** 让 Provider 决定 Run 是否重试、compact、切模型或失败。
- **NEVER** 用字符串匹配作为跨 BC 的主要错误分类契约。
- **MUST** 保证每次 invoke 至多一个终结态，且终结后无事件。
- **MUST** 把 provider tool-call ID 当作边界标识；领域 ToolCallId 由 Runtime 管理。
- **MUST** 对日志和错误做密钥、认证 header 与敏感 body 清洗。
- **MUST** 让取消覆盖请求发送、等待响应、流读取与本地解码等待。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model-and-acl.md](01-domain-model-and-acl.md) | 调用边界语言、模型能力、reasoning clamp、driver 与双向 ACL、不变量 |
| [02-ports-stream-and-client-scope.md](02-ports-stream-and-client-scope.md) | ProviderPort、流式与错误语义、取消、重试边界、不可变 transport/invocation scope |

## 8. 相关文档

- Agent Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Agent Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：锁定 Provider ACL、调用尝试边界、reasoning/usage 所有权与无共享可变状态原则 | #788 |
