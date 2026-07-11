# 03-engineering · 横切工程

> 层级：03-engineering（横切工程关注点）
> 状态：占位承接（S1 建立，S2+ 填充）｜Milestone：v0.1.0
> 本层承载**跨模块的横切关注点**：架构守卫、agent 工程方法论、reasoning graph、可观测性、迁移治理等。不属于单一 BC 的知识落在这里。

## 承接范围

| 计划文档 | 承接现有文档 | 内容 |
|---|---|---|
| `architecture-guards` | `../02-architecture-guards.md`（S1 保持原位，S2/S7 再迁） | 守卫注册表、17 guard 脚本、依赖铁律拦截 |
| `agent-engineering` | `../05-agent-orchestration.md` | Agent 工程五主线（Context/Harness/Loop/Workflow/Graph）、现状评估、演进决策框架 |
| `reasoning-graph` | `../06-agent-reasoning-graph.md` | reasoning effort 阶段调节、ReasoningLevel、provider 映射（归 Workflow BC 的工程侧） |
| `observability` | — | 日志 schema、target 路由、诊断 |
| `migration-governance` | — | Current→Target 迁移治理、退役清单 |

## 待处理事项（迁移时解决）

- **`02-architecture-guards.md` 迁移时机**：CLAUDE.md 触发表当前引用该路径，S1 **保持原位**，S2/S7 迁移时同步更新触发表，避免断链。
- **`06-reasoning-graph` 的 doc-vs-code 分歧**：文档 §2.5 定稿为"runtime 信号检测"，代码实为"LLM 声明 phase"——迁移前 MUST 对齐（以哪个为准由 S2 决策）。
- **`06` README 状态滞后**：标"草案（代码未实现）"，实际 reasoning graph 已落地，迁移时修正为"演进中 / 已落地"。

## 相关文档

- 系统级总体设计：`../01-system/`
- 模块级设计：`../02-modules/`
