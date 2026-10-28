# Provider 根面 Published Language 设计图

`index.html` 是自包含交互设计图（浏览器直接打开）。点击六个领域节点之一，
右侧面板先展示该分类的完整链路（groupChains），再列出该组每个根导出
符号的：类型、职责、定义签名与位置、字段、方法、消费者模块，以及消费
者的调用源码（▶ 标出 AST 命中的那一行）。

## 分组

| 节点 | 内容 |
|---|---|
| 输入 | `ProviderRequestData`（单载体：model/预算/reasoning/整段 system prompt/缓存前缀分界/messages/wire-ready tools/取消令牌） |
| 输出 | 流式 `ProviderResponseChunk`（Content/Usage/Stop/Error 帧）→ `ProviderResponseStream`、非流式 `ProviderResponse`、载荷 `ProviderContentData`/`ResponseStopReason`/`TokenUsageData` |
| 模型元数据 | `ModelInfo`（身份+能力单实体：config/catalog 投影 + `supported_reasoning` 阶梯与 `resolve_reasoning` clamp） |
| 错误 | `ProviderError` + `ProviderErrorKind`（13 变体；`LlmError` 已内部化折叠） |
| 工厂·客户端 | `wire_provider_client`（主链路，返回修正版 ModelInfo）、`wire_test_provider_client`（探测）、`LlmClient`/`ProviderClientSpecData`/`TransportPool`（构造面豁免） |
| 文本端口 | `LlmTextInvokePort`/`LlmTextOutputData`（非流式纯文本调用） |

## 数据来源

- 符号清单：`agent/features/provider/src/lib.rs` 的根 `pub use` 导出面（含
  `composition` 子发布模块与 `use 真名 as 别名` 发布）。
- 消费者与调用位置：`xtask pl-collect`（syn 解析全仓 `*.rs`），只认
  `ExprCall`（调用）、`ExprStruct`（构造）、类型位置 `TypePath`/`TraitBound`
  （类型引用）、`ItemImpl` trait 路径（接口实现）、裸路径 `ExprPath`
  （路径引用，覆盖枚举变体 match 臂）节点；`use`/`pub use` 整棵跳过，
  因此 re-export 不会被误算为消费；排除 provider crate 自身文件。
- 职责（duty）、分组、groupChains 为人工编辑内容。

## 重新生成

```bash
# 1. 采集符号数据（输出含 consumers/snippets 的 payload）
cargo run -p xtask -- pl-collect agent/features/provider . > /tmp/provider-pl-payload.json
#    人工补 duty / groups / nodeGroup / groupChains 后落为 symbol-data.json

# 2. 构建自包含 index.html（archify 底图 + 注入面板）
python3 tools/diagram/build_diagram.py docs/design/02-modules/provider/diagrams/published-language-map
```

`symbol-data.json` 是生成 `index.html` 时使用的完整数据快照，可用于对照或复现。
