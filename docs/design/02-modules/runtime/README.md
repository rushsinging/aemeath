# Agent Runtime（核心域）

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> Agent Runtime 是唯一核心域 BC：驱动"推理 → 工具 → 观察"循环、维护单一 Run 状态机、编排模型调用与工具执行、派生与执行 SubAgent。

## 三元组速览

| 概念 | 回答 | 性质 |
|---|---|---|
| **RunSpec** | 跑什么（prompt/tools/model/timeout/资源模式）| 声明式、可序列化 |
| **RuntimeContext** | 用什么资源跑（供应能力 OHS + Runtime-owned detail ports + config）| 装配的活资源 |
| **Run** | 一次执行实例 | 内存态聚合 + 唯一 Agent 执行生命周期状态机 |

因果链：`RunSpec ──装配──▶ RuntimeContext ──注入──▶ Run`；层级 `Session → Run → Run Step`。

## 核心设计约束

1. **单执行生命周期状态机**：全系统只有 `Run` 驱动 Agent 执行生命周期（内存态、不持久化、崩溃从头开始）；其他 BC 可拥有不驱动 Run 的局部状态机
2. **Loop Engine 零分支**：Main/Sub 共用一套 Loop，差异 100% 在 RunSpec + RuntimeContext + Event adapter
3. **递归能力组织**：仓库 `agent/features/*` 是 VSA；Runtime 内已有多个具备独立词汇、变化原因、状态或测试边界的能力，继续按 `agent_run`、`loop_engine`、各 coordination 等能力竖切
4. **叶子按证据塑形**：`agent_run` 维护领域模型与不变量；其他叶子只在存在真实 seam 时定义就近 Port/adapter，纯编排或投影保持扁平，**NEVER** 复制固定横向层
5. **唯一生产装配**：具体实现选择、factory 调用和对象图连接全部位于 `agent/composition`，Runtime 内不建立第二个 Composition Root
6. **安全铁律**：Sub 能力 ≤ Main（只削弱不越权）
7. **防 stuck 内置**：StuckGuard 四层防线 Main/Sub 统一保护
8. **无 durable**：恢复语义=从头开始，仅保留对话历史快照

## 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Run 聚合、RunSpec、RuntimeContext、实体/VO、不变量、控制权矩阵、安全铁律、差异矩阵 |
| [02-module-boundaries.md](02-module-boundaries.md) | 8 个内部模块、状态所有权、依赖方向 |
| [03-loop-and-state-machine.md](03-loop-and-state-machine.md) | Run 单状态机、Loop Engine 零分支骨架、单 Run vs Session 多 Run 序列 |
| [04-stuck-prevention.md](04-stuck-prevention.md) | StuckGuard 四层防线、分级响应、状态机集成 |
| [05-recovery-semantics.md](05-recovery-semantics.md) | 从头开始恢复、持久化边界、无 durable |
| [06-ports-and-adapters.md](06-ports-and-adapters.md) | 入站 OHS、Runtime 消费的能力契约、RuntimeContext 装配、Composition Root |

## 相关文档

- Session 聚合（属 Context Management）：[../context-management/01-session.md](../context-management/01-session.md)
- 系统级总体设计：[../../01-system/](../../01-system/)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：模块入口 + 三元组速览 + 文档导航 | #761 |
| 2026-07-12 | 出站端口数量改为开放表述，适配 Tool Catalog/Execution 拆分 | #787 |
| 2026-07-15 | 明确 `features/*` 与 Runtime 内部均按能力递归竖切；叶子按领域规则与真实 seam 证据塑形，并保留 `agent/composition` 唯一生产装配边界 | [#995](https://github.com/rushsinging/aemeath/issues/995) |
