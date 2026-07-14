# TUI · 模块总览

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [architecture-and-dataflow.md](01-architecture-and-dataflow.md) | 八层 TEA 管线、三条信息流、3+1 Context Model、Msg/Intent/Change/Effect 枚举、ViewAssembler/ViewModel/ViewState、SDK DTO 边界、架构门禁、死代码清单、reducer 纯化目标态 |
| 02 | [model.md](02-model.md) | 3+3 Context 完整字段、RunStatus/RunStepStatus/ToolCallStatus/SpinnerPhase/AskUserPhase 投影状态机、SpinnerPhase 派生函数、RunRuntimeState 6 子模块、ConfigProjection、WorkspaceProjection、单一真相规则、Model 纯净性约束、现状缺口 |
| 03 | [event-flow-and-acl.md](03-event-flow-and-acl.md) | 事件流完整链路、AgentEventMapper ACL（两层转换 + sanitize）、SDK DTO 边界（convert.rs 漂移 + UiEvent 类型泄漏）、agent_id 缺口 R8、sub-agent 事件路由 #612、转换集中化、架构门禁 #6、现状缺口 11 项 |
| 04 | [view-layer.md](04-view-layer.md) | 10 种 block 类型、ViewAssembler 组装规则、OutputViewCache memo、ViewState 状态机（滚动/选区/折叠/动画）、三层缓存（BlockCache/GuttedCache/force_repaint）、Render 管线、选区复制、Catppuccin 主题、Effect 副作用、架构门禁、死代码清单 |
| 05 | [e2e-scenario-testing.md](05-e2e-scenario-testing.md) | 基于 ratatui TestBackend、crossterm 与 insta 的进程内 E2E 场景测试边界、单帧驱动器、Harness、Effect Driver、确定性约束、快照治理、P0/P1 场景矩阵与 CI 门禁 |

## 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 `AgentClient` trait（SDK 出站端口）与 Runtime 通信
- 不承载业务逻辑——纯展示层
- 基于 The Elm Architecture（TEA）变体

## 相关文档

- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
