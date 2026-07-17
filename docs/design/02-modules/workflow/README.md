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
- v0.2.0 再完善完整 Workflow 能力；不得为远期能力提前创建空层

## Target 物理目录

Workflow 采用 Hexagonal + Clean 最简形态（`domain` only）。v0.1.0 只有 Reasoning Graph / effort 调节这一项稳定能力；node transition、observation、user-maximum clamp 和 `ReasoningPort` 共同守护同一局部状态机，收在 `domain`：

```text
agent/features/workflow/
├── Cargo.toml
└── src/
    ├── lib.rs                       # 窄 façade：ReasoningPort 与稳定 PL + composition-only wiring
    └── domain.rs                    # 领域策略入口
        domain/
        ├── reasoning_graph.rs       #   node 状态与 transition
        ├── effort.rs                #   effort 推断与 user-maximum clamp
        └── error.rs                 #   仅在多个文件共同消费时存在
```

Workflow 当前没有易变技术外部 detail，故 **NEVER** 预建 `adapters/` 或出站 port。#998 完成独立 crate 的物理抽离后，Runtime 临时经 crate-root façade 直接消费 graph；#855/#920/#921 将其收窄为本节 Target 的 `ReasoningPort` façade，并补齐按需出现的 `effort.rs`。Future 若出现多个拥有独立词汇、状态与测试夹具的 workflow 能力，才重新按代码组织规范评估 `capabilities/` 或 `application/` / `adapters/` 升格。

## 相关文档

- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层：[../config/01-config-layer.md](../config/01-config-layer.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-17 | 明确 Workflow 为独立 BC crate；v0.1.0 仅交付 Reasoning Graph，完整能力延至 v0.2.0 | [#998](https://github.com/rushsinging/aemeath/issues/998) |
| 2026-07-16 | 冻结 Workflow Target 物理目录：Reasoning Graph 单能力扁平，明确不建 `capabilities/` 或无证据 adapter | [#972](https://github.com/rushsinging/aemeath/issues/972) |
