# Workflow · 模块总览

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [reasoning-graph.md](01-reasoning-graph.md) | ReasoningNode 状态机、effort 映射、ReasoningPort OHS、user/model 两阶段 clamp、Workflow 远期方向 |

## 定位

Workflow 是 **支撑域 BC**，独占 reasoning phase、effort 调节与 user-maximum clamp；model-capability clamp 归 Provider resolver：

- ReasoningGraph 根据对话阶段动态调节 reasoning effort
- 通过 Workflow-owned `ReasoningPort` OHS 向 Runtime 发布稳定能力，与 Provider detail 解耦
- v0.1.0 只包含 effort 调节；通用控制流编排不在范围内

## 相关文档

- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层：[../config/01-config-layer.md](../config/01-config-layer.md)
