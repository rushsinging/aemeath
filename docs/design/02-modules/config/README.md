# Config · 模块总览

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [config-layer.md](01-config-layer.md) | Config 分层优先级链、ConfigSnapshot PL、ConfigReader/ConfigAppService、CompatibilityAdapter ACL（外部 CLI 配置兼容层）、adapter 接入、reasoning 静态阈值 |

## 定位

Config 是**通用域 BC**——为所有其他 BC 提供配置真相：

- ConfigSnapshot 是 Published Language，通过 watch channel 推送
- ConfigReader 是出站端口，消费方通过此端口获取配置
- 不包含业务逻辑——只承载配置数据

## 相关文档

- Workflow 战术设计：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
