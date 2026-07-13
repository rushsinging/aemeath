# Agent Runtime（核心域）

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> Agent Runtime 是唯一核心域 BC：驱动"推理 → 工具 → 观察"循环、维护单一 Run 状态机、编排模型调用与工具执行、派生与执行 SubAgent。

## 三元组速览

| 概念 | 回答 | 性质 |
|---|---|---|
| **RunSpec** | 跑什么（prompt/tools/model/timeout/资源模式）| 声明式、可序列化 |
| **RuntimeContext** | 用什么资源跑（出站端口 + config + event sink）| 装配的活资源 |
| **Run** | 一次执行实例 | 内存态聚合 + 唯一状态机 |

因果链：`RunSpec ──装配──▶ RuntimeContext ──注入──▶ Run`；层级 `Session → Run → Run Step`。

## 核心设计约束

1. **单状态机**：全系统只有 `Run`（内存态、不持久化、崩溃从头开始）
2. **Loop Engine 零分支**：Main/Sub 共用一套 Loop，差异 100% 在 RunSpec + RuntimeContext + Event adapter
3. **安全铁律**：Sub 能力 ≤ Main（只削弱不越权）
4. **防 stuck 内置**：StuckGuard 四层防线 Main/Sub 统一保护
5. **无 durable**：恢复语义=从头开始，仅保留对话历史快照

## 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Run 聚合、RunSpec、RuntimeContext、实体/VO、不变量、控制权矩阵、安全铁律、差异矩阵 |
| [02-module-boundaries.md](02-module-boundaries.md) | 8 个内部模块、状态所有权、依赖方向 |
| [03-loop-and-state-machine.md](03-loop-and-state-machine.md) | Run 单状态机、Loop Engine 零分支骨架、单 Run vs Session 多 Run 序列 |
| [04-stuck-prevention.md](04-stuck-prevention.md) | StuckGuard 四层防线、分级响应、状态机集成 |
| [05-recovery-semantics.md](05-recovery-semantics.md) | 从头开始恢复、持久化边界、无 durable |
| [06-ports-and-adapters.md](06-ports-and-adapters.md) | 入站端口、出站端口、RuntimeContext 装配、Composition Root |

## 相关文档

- Session 聚合（属 Context Management）：[../context-management/01-session.md](../context-management/01-session.md)
- 系统级总体设计：[../../01-system/](../../01-system/)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：模块入口 + 三元组速览 + 文档导航 | #761 |
| 2026-07-12 | 出站端口数量改为开放表述，适配 Tool Catalog/Execution 拆分 | #787 |
