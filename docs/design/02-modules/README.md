# 02-modules · 模块级设计

> 层级：02-modules（模块 / BC 战术设计）
> 状态：Target｜Milestone：v0.1.0（S2 #761 填充实质内容）
> 本层承载各 Bounded Context 的**战术设计**：聚合、实体、值对象、不变量、领域服务、模块内端口与内部结构。**只描述目标态。** 总体战略设计见 [../01-system/](../01-system/)。

## 模块文档

每个 BC / 模块一份文档，用数字前缀命名：

| 目标文档 | 内容 | 状态 |
|---|---|---|
| [runtime/](runtime/README.md) | Run 聚合、单状态机、Loop Engine、防 stuck、恢复语义、端口与装配 | ✅ S2 |
| [context-management/](context-management/01-session.md) | Session 聚合、Compact 家族（五级管线）、Token Budget、Prompt/Guidance、Memory 注入 | ✅ S2 |
| [tools/](tools/README.md) | Tool Catalog/Execution 双端口、Scope/Profile、Skill、Slash Command 与 MCP 生命周期 | ✅ S2 |
| memory / task / project / policy / audit | 各支撑 BC 战术设计 | 规划 |
| [provider/](provider/README.md) | Provider ACL、统一调用流、模型能力、reasoning 映射与不可变 Invocation Scope | ✅ S2 |
| [workflow/](workflow/README.md) | ReasoningGraph 节点状态机、effort 调节、ReasoningPort OHS、clamp 统一、Workflow 远期方向 | ✅ S2 |
| [config/](config/README.md) | Config 分层优先级链、ConfigSnapshot PL、ConfigReader/ConfigAppService、adapter 接入、reasoning 静态阈值 | ✅ S2 |
| tui | TEA 架构、四 Context、DTO 边界、守卫 | 规划 |
| [server/](server/README.md) | WSS 协议、控制面 / worker 拓扑 | ⏸ 占位（#794 暂缓） |

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
| 2026-07-12 | 新增 tools/ 战术设计：Tool 双端口、Scope/Profile、Skill/Command 与 MCP 生命周期 | #787 |
| 2026-07-12 | 新增 provider/ 战术设计：ProviderPort、ACL、流语义、模型能力与 Invocation Scope | #788 |
| 2026-07-12 | 新增 context-management/ 02-05：Compact 家族、Token Budget、Prompt/Guidance、Memory 注入 | #786 |
| 2026-07-12 | 新增 workflow/ 与 config/ 战术设计：ReasoningGraph、ReasoningPort、Config 分层、ConfigSnapshot PL | #792 |
| 2026-07-12 | 新增 server/ 占位文档：暂缓设计，继承草案约束 | #794 |
