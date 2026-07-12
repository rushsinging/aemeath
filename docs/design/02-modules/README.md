# 02-modules · 模块级设计

> 层级：02-modules（模块 / BC 战术设计）
> 状态：Target｜Milestone：v0.1.0（S2 #761 填充实质内容）
> 本层承载各 Bounded Context 的**战术设计**：聚合、实体、值对象、不变量、领域服务、模块内端口与内部结构。**只描述目标态。** 总体战略设计见 [../01-system/](../01-system/)。

## 模块文档

每个 BC / 模块一份文档，用数字前缀命名：

| 目标文档 | 内容 | 状态 |
|---|---|---|
| [runtime/](runtime/README.md) | Run 聚合、单状态机、Loop Engine、防 stuck、恢复语义、端口与装配 | ✅ S2 |
| [context-management/](context-management/01-session.md) | Session 聚合（已填充）；compact 家族 / token budget / prompt 待补 | 🚧 部分 |
| memory / task / project / policy / audit / tools | 各支撑 BC 战术设计 | 规划 |
| provider | Provider ACL、driver 映射 | 规划 |
| tui | TEA 架构、四 Context、DTO 边界、守卫 | 规划 |
| server | WSS 协议、控制面 / worker 拓扑（v0.1.0 之后） | 规划 |

## 编写原则

- 只描述目标态，区分 Target / Decision，不记录当前代码状态。
- 每篇独立成文，带"相关文档"链接与"修改历史"。

## 相关文档

- 系统级总体设计：[../01-system/](../01-system/)
- 横切工程：[../03-engineering/README.md](../03-engineering/README.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：承接说明 + 规划模块清单 | #760 |
| 2026-07-11 | 改为纯目标态（移除"承接现有文档"迁移列）、链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run | #760 |
| 2026-07-11 | S2 填充 runtime/（7 篇）与 context-management/session.md，规划表改链接 | #761 |
