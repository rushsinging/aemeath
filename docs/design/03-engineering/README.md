# 03-engineering · 横切工程

> 层级：03-engineering（横切工程关注点）
> 状态：Target｜Milestone：v0.1.0（S2+ 填充实质内容）
> 本层承载**跨模块的横切关注点**：架构守卫、agent 工程方法论、reasoning graph、可观测性、迁移治理等。不属于单一 BC 的知识落在这里。**设计类文档只描述目标态；迁移治理文档专门承载过渡期的 Current→Target 追踪。**

## 规划的横切文档

| 目标文档 | 内容 |
|---|---|
| architecture-guards | 守卫注册表、依赖铁律拦截 |
| agent-engineering | Agent 工程五主线（Context / Harness / Loop / Workflow / Graph）、演进决策框架 |
| reasoning-graph | reasoning effort 阶段调节、ReasoningLevel、provider 映射 |
| observability | 日志 schema、target 路由、诊断 |
| migration-governance | Current→Target 迁移追踪、旧文档去向、退役清单（**所有过渡期状态集中于此，避免污染设计文档**） |

## 编写原则

- 设计类文档只描述目标态，带"相关文档"链接与"修改历史"。
- **迁移治理**是唯一允许记录 Current 状态的文档，用于追踪旧路径 / 死代码 / 待退役项。

## 相关文档

- 系统级总体设计：[../01-system/](../01-system/)
- 模块级设计：[../02-modules/README.md](../02-modules/README.md)
- 架构守卫注册表：[../02-architecture-guards.md](../02-architecture-guards.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：承接说明 + 待处理事项 | #760 |
| 2026-07-11 | 移除 current 迁移问题描述、把过渡状态收敛到 migration-governance、链接化、新增修改历史 | #760 |
