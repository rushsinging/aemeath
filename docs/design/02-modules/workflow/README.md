# Workflow · 模块总览

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#921](https://github.com/rushsinging/aemeath/issues/921)
> **v0.1.0 scope 收缩**：Reasoning Graph 领域模型与五节点固定默认 effort 已交付；Config `reasoning_graph` 已退役；Provider resolver 领域迁移完成但未接生产链路。Main 已接线消费 ReasoningPort；Runtime/Context/TUI **尚未**端到端消费 Provider resolver。是否保留/接线由 v0.2.0 [#1142](https://github.com/rushsinging/aemeath/issues/1142) 决策。

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [reasoning-graph.md](01-reasoning-graph.md) | ReasoningNode 状态机、effort 映射、ReasoningPort OHS、user/model 两阶段 clamp、Workflow 远期方向 |

## 定位

Workflow 是 **支撑域 BC**，独占 reasoning phase、effort 调节与（Target 中）user-maximum clamp；model-capability clamp 归 Provider resolver：

- ReasoningGraph 根据对话阶段动态调节 reasoning effort
- 通过 Workflow-owned `ReasoningPort` OHS 向 Runtime 发布稳定能力，与 Provider detail 解耦
- v0.1.0 只包含五节点固定默认 effort（无 config override；Config `reasoning_graph` 已退役）；通用控制流编排不在范围内
- v0.2.0 再完善完整 Workflow 能力；不得为远期能力提前创建空层
- **#921 收缩**：Provider resolver 领域迁移完成但未接生产链路；Runtime/Context/TUI 尚未接线；是否保留/接线由 v0.2.0 #1142 决策

## Target 物理目录

Workflow 采用 Hexagonal + Clean 最简形态（`domain` only）。v0.1.0 交付五节点固定默认 effort 这一项稳定能力（Config `reasoning_graph` 已退役，#921）；node transition、observation 和 `ReasoningPort` 共同守护同一局部状态机，收在 `domain`。**v0.1.0 scope**：Main adaptive ReasoningPort 已接入生产 loop，但 Provider resolver 未接线：

```text
agent/features/workflow/
├── Cargo.toml
└── src/
    ├── lib.rs                       # 窄 façade：ReasoningPort 与稳定 PL + composition-only wiring
    └── domain.rs                    # 领域策略入口
        domain/
        ├── reasoning_graph.rs       #   node 状态与 transition（私有）
        └── reasoning_port.rs        #   adaptive Port 与 observation（user-max clamp 已退役，#921）
```

Workflow 当前没有易变技术外部 detail，故 **NEVER** 预建 `adapters/` 或出站 port。#998 完成独立 crate 抽离，#919 冻结 graph 语义，#920 将 Main 收口为 Workflow-owned adaptive `ReasoningPort`。Sub Fixed/Inherit/NoOp 随 #875/#878 的统一 RuntimeContext 生产接线落地，避免为即将退役的 legacy Sub 创建一次性适配；Future 若出现多个拥有独立词汇、状态与测试夹具的 workflow 能力，才重新评估升格。

## 相关文档

- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层：[../config/01-config-layer.md](../config/01-config-layer.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-17 | Main 通过 adaptive ReasoningPort 消费 graph；Sub Fixed/Inherit/NoOp 延至统一 RuntimeContext 接线 | [#920](https://github.com/rushsinging/aemeath/issues/920) |
| 2026-07-17 | 明确 Workflow 为独立 BC crate；v0.1.0 仅交付 Reasoning Graph，完整能力延至 v0.2.0 | [#998](https://github.com/rushsinging/aemeath/issues/998) |
| 2026-07-16 | 冻结 Workflow Target 物理目录：Reasoning Graph 单能力扁平，明确不建 `capabilities/` 或无证据 adapter | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #921 收缩范围：Config `reasoning_graph` 退役，五节点固定默认 effort；Provider resolver 领域迁移完成但未接生产链路；Runtime/Context/TUI 尚未接线；是否保留/接线由 v0.2.0 #1142 决策 | [#921](https://github.com/rushsinging/aemeath/issues/921) |
