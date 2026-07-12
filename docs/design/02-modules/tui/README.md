# TUI · 模块总览

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [architecture-and-dataflow.md](01-architecture-and-dataflow.md) | 八层 TEA 管线、三条信息流、3+1 Context Model、Msg/Intent/Change/Effect 枚举、ViewAssembler/ViewModel/ViewState、SDK DTO 边界、架构门禁、死代码清单、reducer 纯化目标态 |
| 02 | [model.md](02-model.md) | 3+1 Context 完整字段、ChatStatus/ChatTurnStatus/ToolCallStatus/SpinnerPhase/AskUserPhase 投影状态机、RuntimeState 8 子模块、单一真相规则、Model 纯净性约束、现状缺口 |

## 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 `AgentClient` trait（SDK 出站端口）与 Runtime 通信
- 不承载业务逻辑——纯展示层
- 基于 The Elm Architecture（TEA）变体

## 相关文档

- 原始 TUI 设计（历史归档）：[../../04-tui-design.md](../../04-tui-design.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
