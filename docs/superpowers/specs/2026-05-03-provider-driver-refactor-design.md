# Provider Driver 重构设计

日期：2026-05-03

## 背景

当前项目中 `provider` 同时承担了两种含义：

1. 用户配置中的 `models.providers` key，例如 `Zhipu`、`LiteLLM`、`CompanyProxy`。
2. 代码层真正理解的 API 类型，例如 Anthropic、OpenAI-compatible、Zhipu、LiteLLM。

这导致外部 CLI 暴露 `--provider`，同时内部又通过 provider name 字符串推断请求体行为，例如用 `provider_name == "zhipu"` 决定是否发送 Zhipu `thinking` 字段。该设计会让配置 key 与协议/厂商语义耦合，也会让 LiteLLM 的 `<provider>/<model>` 模型名与 aemeath 的 `provider/model` 选择格式混淆。

本次重构目标是：用户配置 key 只表示自定义模型来源；代码层只通过配置中的 `api` 字段选择 API driver。

## 目标

- 移除对外 `--provider` 参数。
- 移除 `AEMEATH_PROVIDER` 语义。
- `--model` 与 `AEMEATH_MODEL` 语义完全一致，均使用 `<source-key>/<model-query>` 格式。
- `models.providers` 的 key 仅作为用户自定义模型来源名称。
- `api` 字段成为代码层唯一可理解的 driver 类型。
- `api` 支持值限定为：`anthropic`、`openai`、`zhipu`、`litellm`。
- 移除对外和内部新逻辑中的 `openai-compatible` API 类型。
- 引入已解析模型结构 `ResolvedModel`。
- 引入 OpenAI Chat Completions 路径内部的 `ChatApiDriver` 策略接口。
- OpenAI、Zhipu、LiteLLM 共享 Chat Completions transport，但各自处理 endpoint、reasoning 请求体和后续扩展点。

## 非目标

本次不实现：

- LiteLLM cost header 解析。
- Zhipu 超大 body 自动截断。
- Zhipu 空 usage 响应重试。
- config schema 字段重命名，例如把 `models.providers` 改成 `models.sources`。
- 旧配置或旧 CLI 参数自动迁移。
- `--provider` 兼容 warning。

## 外部语义

### CLI

移除：

```bash
--provider
AEMEATH_PROVIDER
```

保留：

```bash
--model
AEMEATH_MODEL
```

`--model` 和 `AEMEATH_MODEL` 的值必须使用完整模型选择格式：

```text
<source-key>/<model-query>
```

示例：

```bash
aemeath --model Zhipu/glm-5.1
aemeath --model LiteLLM/anthropic/claude-opus-4-7
aemeath --model OpenAI/gpt-5.5
```

其中：

- `Zhipu`、`LiteLLM`、`OpenAI` 是 `config.json` 中 `models.providers` 的 key。
- 这些 key 是用户自定义模型来源，不是代码层 provider。
- 代码层 driver 由该 source 配置中的 `api` 字段决定。

如果用户传入 `--provider`，clap 直接报未知参数。若环境中存在 `AEMEATH_PROVIDER`，程序忽略它。

### config

保留现有结构：

```json
{
  "models": {
    "default": "Zhipu/glm-5.1",
    "providers": {
      "Zhipu": {
        "api": "zhipu",
        "baseUrl": "https://api.z.ai/api/coding/paas/v4",
        "apiKey": "...",
        "models": [
          { "id": "glm-5.1", "reasoning": true }
        ]
      }
    }
  }
}
```

代码语义上，`models.providers` 的 key 解释为模型来源配置项。

## API Driver

### Driver 类型

替换现有 `ApiType::OpenAICompatible` 语义，收敛为：

```rust
enum ApiDriverKind {
    Anthropic,
    OpenAI,
    Zhipu,
    LiteLLM,
}
```

映射规则：

| config `api` | driver |
|---|---|
| `anthropic` | `ApiDriverKind::Anthropic` |
| `openai` | `ApiDriverKind::OpenAI` |
| `zhipu` | `ApiDriverKind::Zhipu` |
| `litellm` | `ApiDriverKind::LiteLLM` |

`openai-compatible` 不再是合法的新配置值。

`api: "openai"` 表示使用 OpenAI Chat Completions 协议，不代表一定是 OpenAI 官方服务。官方 OpenAI、自建代理、兼容服务都可以使用 `api: "openai"`。

### LlmProvider 与 ChatApiDriver

`LlmProvider` 是 agent 调用 LLM 的完整后端接口，负责 streaming、non-streaming、message/tool 转换、response 解析、usage、错误处理等。

`ChatApiDriver` 是 OpenAI Chat Completions transport 内部的策略接口，只处理 `openai`、`zhipu`、`litellm` 的差异：

- endpoint suffix
- request body patch
- reasoning 字段策略
- header 扩展点
- response/usage 特化扩展点

关系：

```text
Agent
  ↓
LlmClient
  ↓
dyn LlmProvider
  ↓
OpenAiChatProvider
  ↓
dyn ChatApiDriver
      ├── OpenAiDriver
      ├── ZhipuDriver
      └── LiteLlmDriver
```

Anthropic 保持独立 `LlmProvider` 路径，不纳入 `ChatApiDriver`。

## 模型解析

新增 `ResolvedModel`，作为启动和模型切换后的唯一解析结果：

```rust
struct ResolvedModel {
    source_key: String,
    source_config: ProviderModelsConfig,
    model: ModelEntryConfig,
    api: ApiDriverKind,
}
```

后续可逐步把 `ProviderModelsConfig` 重命名为 `ModelSourceConfig`，但本次不强制改 schema 和所有类型名。

### 解析优先级

```text
--model > AEMEATH_MODEL > config.models.default
```

三者值均使用：

```text
<source-key>/<model-query>
```

解析规则：

1. 只按第一个 `/` 分割，兼容 LiteLLM 模型 ID 中包含 `/`。
2. `/` 前半部分大小写不敏感匹配 `models.providers` key。
3. `/` 后半部分作为 model query。
4. 先精确匹配 `model.name`。
5. 再精确匹配 `model.id`。
6. 最后使用现有 normalized fuzzy match。

示例：

```text
LiteLLM/anthropic/claude-opus-4-7
```

解析为：

```text
source_key = LiteLLM
model_query = anthropic/claude-opus-4-7
```

### 默认模型

如果没有 `--model`、没有 `AEMEATH_MODEL`、且 `models.default` 为空：

- 若配置中只有一个 source 且只有一个 model，可以自动选择。
- 否则报错，提示用户设置：

```bash
aemeath --model <source>/<model>
```

### 错误信息

source 不存在：

```text
未找到模型来源 'X'。
可用来源：
  Zhipu
  LiteLLM
  OpenAI
```

model 不存在：

```text
来源 'Zhipu' 中未找到模型 'glm-x'。
可用模型：
  glm-5.1
  glm-4.5
```

## 请求体与 reasoning 策略

模型配置继续支持：

```json
"reasoning": true
```

和：

```json
"reasoning": { "effort": "medium" }
```

### api = openai

基础 endpoint：

```text
/v1/chat/completions
```

reasoning 规则：

- `reasoning: true`：只表示内部启用，不额外发送字段。
- `reasoning: false`：不发送 reasoning 字段。
- `reasoning: { "effort": "medium" }`：发送：

```json
"reasoning": { "effort": "medium" }
```

模型名支持检查只作为 warning/debug，不阻止发送。

### api = zhipu

endpoint：

```text
/chat/completions
```

reasoning 规则：

- `reasoning: true` 或 object：发送：

```json
"thinking": { "type": "enabled" }
```

- `reasoning: false`：发送：

```json
"thinking": { "type": "disabled" }
```

- 未配置 reasoning：沿用 `--no-think` 的默认开关，再转成 `thinking.type`。

object 中的 `effort` 暂时忽略，不报错。

### api = litellm

endpoint：

```text
/v1/chat/completions
```

reasoning 规则：

- `reasoning: true`：不发送 reasoning 字段。
- `reasoning: false`：不发送 reasoning 字段。
- `reasoning: { "effort": "medium" }`：透传：

```json
"reasoning": { "effort": "medium" }
```

本次不实现 LiteLLM 其他专属字段。

### api = anthropic

保持现有 Anthropic provider 路径和现有 reasoning 行为。

### --reasoning-effort

保留 CLI 参数，仅影响 `openai` 和 `litellm` 的 object reasoning：

```text
--reasoning-effort > config model.reasoning.effort > AEMEATH_REASONING_EFFORT
```

如果当前 api 是 `zhipu`，`--reasoning-effort` 不报错，只记录 debug/warn：Zhipu 当前只支持 thinking on/off。

## TUI `/model`

`/model list` 展示完整模型选择：

```text
Zhipu/glm-5.1
LiteLLM/anthropic/claude-opus-4-7
OpenAI/gpt-5.5
```

`/model <query>` 推荐使用相同格式：

```text
/model Zhipu/glm-5.1
```

可以保留现有 fuzzy 体验，但最终必须解析为 `ResolvedModel`，并通过 `api` 字段创建 driver。

## 测试策略

### core

- `ApiDriverKind::from_str("openai")` 正常。
- `ApiDriverKind::from_str("zhipu")` 正常。
- `ApiDriverKind::from_str("litellm")` 正常。
- `ApiDriverKind::from_str("openai-compatible")` 返回错误/None。
- `ModelSelector::resolve("Zhipu/glm-5.1")` 正常。
- `ModelSelector::resolve("LiteLLM/anthropic/claude-opus-4-7")` 正常。
- source 不存在时返回中文错误并列出来源。
- model 不存在时返回中文错误并列出模型。

### llm

- OpenAI object reasoning 发送 `reasoning`。
- OpenAI bool reasoning 不发送 `reasoning`。
- Zhipu bool reasoning 发送 `thinking.type`。
- Zhipu object reasoning 发送 `thinking.type = enabled`。
- LiteLLM object reasoning 透传 `reasoning`。
- LiteLLM bool reasoning 不发送 `reasoning`。

### cli/tui

- CLI 不存在 `--provider`。
- `AEMEATH_MODEL` 与 `--model` 解析结果一致。
- `/model Zhipu/glm-5.1` 能切换。
- `/model LiteLLM/anthropic/claude-opus-4-7` 能切换。

## 验证门禁

完成实现后运行：

```bash
cargo fmt
cargo test -p aemeath-core
cargo test -p aemeath-llm
cargo test -p aemeath-cli
cargo check
```

若 CLI 全量测试成本过高，至少运行相关模块测试与 `cargo check`。

## Feature 状态

本设计覆盖并扩大 Feature #19 与 Feature #20：

- #19：Zhipu API 类型从配置 `api` 显式驱动。
- #20：LiteLLM API 类型从配置 `api` 显式驱动。

实现期间将 `docs/feature/active.md` 中 #19/#20 标记为实现中。完成后等待用户确认，再按项目规则归档。
