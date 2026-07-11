# 02-modules · 模块级设计

> 层级：02-modules（模块 / BC 战术设计）
> 状态：占位承接（S1 建立，S2 #761 填充）｜Milestone：v0.1.0
> 本层承载各 Bounded Context 的**战术设计**：聚合、实体、值对象、不变量、领域服务、模块内端口与内部结构。总体战略设计见 `../01-system/`。

## 承接范围

本层每个 BC / 模块一份文档（S2 起用数字前缀命名，如 `01-runtime.md`）。规划：

| 计划文档 | 承接现有文档 | 内容 |
|---|---|---|
| `runtime`（Agent Execution + Workflow） | `../03-runtime-design.md` | AgentRun 聚合 + 状态机、Loop Engine、各 Coordinator、reasoning graph |
| `context-management` | 从 `03-runtime-design.md` 抽出 | Session 聚合、compact 家族、token budget、注入、prompt |
| `memory` / `task` / `project` / `policy` / `audit` / `tools` | 分散现状 | 各支撑 BC 战术设计 |
| `provider` | — | Provider ACL、driver 映射 |
| `tui` | `../04-tui-design.md` | TEA 架构、四 Context、DTO 边界、守卫 |
| `server` | `../07-server-design.md` | WSS 协议、控制面 / worker 拓扑（v0.1.0 之后） |

## 现有文档迁移原则

- S1 阶段**只标去向、不移动文件**（保护引用与历史）。
- 真正拆分 / 迁移在 S2（设计文档）与 S5（代码模块）执行。
- 迁移时区分 Current / Target / Migration / Decision 四态。

## 相关文档

- 系统级总体设计：`../01-system/`
- 横切工程：`../03-engineering/`
