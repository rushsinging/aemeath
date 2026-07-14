# TUI · 模块总览

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [architecture-and-dataflow.md](01-architecture-and-dataflow.md) | 八层 TEA 管线、三条信息流、六个 Context / Projection、Msg / Intent / Change / Effect、ViewAssembler / ViewModel / ViewState、SDK DTO 边界与架构门禁 |
| 02 | [model.md](02-model.md) | Conversation / Input / Diagnostic / Session / Config / Workspace 私有核心投影、Runtime 权威状态机、runs / timeline 互补投影、四类 Interaction request-id 状态与 Model 纯净性约束 |
| 03 | [event-flow-and-acl.md](03-event-flow-and-acl.md) | 唯一 SDK event → TUI DTO → Intent → Change → Effect → result Intent 链、两层 ACL、六 Context 穷尽映射、Runtime-owned interaction id / AgentClient reply、agent_id / sub-agent 路由与门禁 |
| 04 | [view-layer.md](04-view-layer.md) | block 类型、ViewAssembler 组装、OutputViewCache memo、ViewState（滚动/选区/折叠/动画）、缓存、Render、选区复制与主题 |

## 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 Runtime-owned `AgentClient` 入站 OHS（由 SDK 发布契约）与 Runtime 通信；从 TUI 视角它是唯一对外依赖
- 不承载业务逻辑——纯展示层
- 基于 The Elm Architecture（TEA）变体
- `UiEvent` **NEVER** 直达 Model：所有 SDK 事件必须经两层 ACL、六 Context Intent、reducer Change、Coordinator Effect 与 result Intent 闭环
- UserQuestions、ToolApproval、PlanApproval、HardPause 共用 Runtime 生成的 Interaction request id，并经 SDK / TUI ACL / AgentClient reply command 无损贯穿；TUI **NEVER** 持有 sender、pending waiter 或自生成协议 id
- Interaction command result 只结束本地交互块；Run 只由 SDK `RunResumed` / `RunCancelling` / `RunCancelled` 等 Runtime 权威事件推进
- 六 Context 核心字段私有，root reducer 是唯一写入口；ViewAssembler 只读 accessor，ViewState 只持瞬时交互 / 渲染状态
- Conversation 的结构化投影（runs / queued / progress）与 `timeline` 是同一 reducer 事务原子维护的互补投影，只约束重叠事实，**NEVER** 假定可完整互相重建

## 相关文档

- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
