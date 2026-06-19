# 设计：tool registry / tool result / 管线重构 —— typed core, JSON only at edges

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/376
> 日期: 2026-06-19
> 状态: 设计已确认，分阶段实施中

## 1. 背景与痛点

当前工具结果从执行到 LLM/TUI 的链路存在以下问题：

1. **位置取值的 6 元组**：`UiToolResult = (ToolCallId, String /*provider_id*/, String /*output*/, serde_json::Value /*content*/, bool, Vec<ImageData>)`，全管线按 `result.0`/`result.3` 取值，可读性差、易错。
2. **wire content 三种不一致形态**：成功=结构化 `data`、错误/取消/拦截=`{"text": msg}`、超长=`{"text":<指针>,"persisted":{…}}`。LLM 收到的东西不可预期。
3. **死字段**：`share::tool::ToolResult { …, data: Option<R> }` 经 adapter 后恒为 `None`。
4. **registry 三函数近重复**：`register_all_tools` / `register_subagent_tools` / `register_all_tools_except_agent`。
5. **管线手动双发**：`send_tool_result`(TUI) + `out.push`(LLM) 在 non_agent / agent_calls / ask_user / denied 各路重复。
6. **input 边界不 typed**：LLM 发来 `Value`，各工具内部各自 `from_value` / 手抠字段；`input_schema()` 手写、与解析两套真相、易漂移。

## 2. 指导原则

**typed core, JSON only at the edges**：过程中一律传 typed object；只有对外的两条边界才落 JSON——
- **LLM 边界**：text-first，只发 `text` 字符串（+ 有图时的多块数组）。
- **TUI / server 边界**：发结构化 JSON（按 `tool_name → Output schema` 反序列化）。

## 3. 最终类型

```rust
// 工具执行结果（统一、具体、非泛型）
pub struct ToolOutcome {
    pub text: String,            // → LLM(text-first) + TUI 预览；LLM 唯一读到的内容
    pub data: serde_json::Value, // → TUI/server，按 tool_name 的 Output schema 反序列化；无结构化时 Null
    pub is_error: bool,
    pub images: Vec<ImageData>,  // 多模态：有图时随 text 组成 wire 多块数组
}

// 管线传递单元，取代 UiToolResult 6 元组
pub struct ToolExecution {
    pub call_id: ToolCallId,
    pub provider_id: String,
    pub tool_name: String,
    pub outcome: ToolOutcome,
}

// 工具 trait：input/output 对称 typed；input_schema 由 schemars 派生（C2）
pub trait TypedTool {
    type Input:  serde::de::DeserializeOwned + schemars::JsonSchema + Send;
    type Output: serde::Serialize + Send;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value { schemars 从 Input 派生 }
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { true }
    fn timeout_secs(&self) -> u64 { 120 }
    async fn call(&self, input: Self::Input, ctx) -> TypedToolResult<Self::Output>;
}
```

adapter 是两条边界的对称网关：`Value →(deserialize)→ Input` 进、`Output →(serialize)→ data` 出。registry 因关联类型 `Input/Output` 非对象安全，仍需 adapter 擦除存 `Box<dyn Tool>`——该擦除点正是「typed core → TUI/server JSON」边界，保留且正当。

## 4. LLM 边界（ContentBlock）

工具结果 → `role=User` 的 `Message`，每个结果一个 `ContentBlock::ToolResult { tool_use_id, content, is_error }`：
- 无图片：`content = Value::String(outcome.text)`（text-first）。
- 有图片：`content = [ {type:image,…}, {type:text, text: outcome.text} ]`（不再塞 json 块）。

`data` 在此**丢弃**，不发 LLM。超长（>50K）由 `persist_oversized_results` 在边界把 `text` 替换为 `<persisted-output>` 指针并落盘。

## 5. 分阶段实施（每阶段一个独立可验证 PR）

| 阶段 | 内容 | 行为变化 | 验收 |
|---|---|---|---|
| **A** | 引入 `ToolOutcome` + `ToolExecution`，管线/agent 用其取代 6 元组；adapter 产 `ToolOutcome`；MCP 直 impl 同步。**保持 wire 行为不变**（LLM 暂仍收 data） | ❌ | build/test/clippy + `cargo run -- -qv` 冒烟，wire 字节不变 |
| **B** | LLM 边界改 **text-first**；统一 content（删 `{text}`/`{status,message}` ad-hoc）；修各工具 `text` 自足（TaskList 等） | ✅ | 单测 wire content；`-qv` 实跑多工具看 LLM 输入日志 |
| **C** | typed `Input` + `schemars` 派生 `input_schema`（C2）；迁移 27 工具入参 + 搬描述 | ✅ | 单测 input 反序列化 + schema；`-qv` 实跑工具调用准确率 |
| **D** | 收敛 3 个 `register_*` 为按能力/过滤的单一注册入口 | ❌ | 单测 subagent 工具集；`-qv` |
| **E** | `tool_name → Input/Output schema` 注册表（server 钩子） | ❌ | 单测 schema 导出 |

A 零行为风险打地基先行；B/C 行为敏感单独验证；D/E 收尾。

## 6. 验收手段（贯穿各阶段）

- **单元测试**：类型映射、wire content 形态、input 反序列化、schema 派生、registry 过滤。
- **实跑**：`echo '<prompt>' | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv`（-q 静默 + -v 日志到 stderr），用真实 prompt 触发工具，查 `agent-tools.log` / `agent-provider.log` 验证 LLM 实际收到的 tool_result 内容。
- **验收脚本**：新增 `scripts/accept-tool-pipeline.sh`，跑构建 + 测试 + clippy + 一组 `-qv` 冒烟用例并断言日志。
- **日志**：在 adapter 擦除点、管线扇出点、LLM/TUI 边界增加结构化 log（target `aemeath:agent:tools` / `aemeath:agent:provider`），便于 `-qv` 验收。
- TUI 渲染层不强测（按目标约定）。

## 7. 非目标（YAGNI）

- 不改 provider HTTP/stream 实现（除 LLM 边界 content 组装）。
- 不引入图片去重 / 多模态新能力（仅保留现状 images 透传）。
- 不动 compact/session 持久化逻辑（除 `persist_oversized_results` 已在边界）。
