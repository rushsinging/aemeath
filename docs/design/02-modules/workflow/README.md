# Workflow · 模块总览

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [reasoning-graph.md](01-reasoning-graph.md) | ReasoningNode 状态机、effort 映射、ReasoningPort OHS、clamp 统一、Workflow 远期方向 |

## 定位

Workflow 模块是 **Runtime 内部的 effort 调节层**，不独立成 BC：

- ReasoningGraph 根据对话阶段动态调节 reasoning effort
- 通过 ReasoningPort 与 Provider BC 解耦
- Phase 3 Workflow Engine（控制流编排）为远期规划，暂缓

## 相关文档

- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层：[../config/01-config-layer.md](../config/01-config-layer.md)
