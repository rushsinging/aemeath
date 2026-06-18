# docs/design/

> 本目录是 aemeath 的**设计真相源**——`outline.md` 给出全局架构原则，各模块设计稿给出详细子域设计，`architecture-guards.md` 给出"机械式宪法"。任何代码或配置的修改，凡涉及架构层面的不变量，**MUST** 同时核对本目录相关文档并保持一致。

## 索引

| 文档 | 角色 | 状态 |
|---|---|---|
| [outline.md](outline.md) | 全局设计总纲：Bounded Context、COLA 分层、依赖铁律、关键约束 | 已落地 |
| [runtime-design.md](runtime-design.md) | 核心域 Runtime：Agent Looping、Token Budget、Compact、Cost、Slash Command | 已落地 |
| [tui-design.md](tui-design.md) | 入站适配器 TUI：六边形边界、Model/View Model/Render、Effect 编排 | 已落地 |
| [server-design.md](server-design.md) | 入站适配器 Server：多租户远端服务、WSS 协议、Session 多路复用 | 草案（无 server crate） |
| [file-split-plan.md](file-split-plan.md) | 文件级切分计划：把巨型文件按 COLA 边界分到 `contract/gateway/business/utils` | 演进中 |
| [architecture-guards.md](architecture-guards.md) | 17 个架构守卫 + 全部白名单的单一真相（与 `.agents/hooks/*.sh` 字面同步） | 已落地 |
| [README.md](README.md) | 本文件 | — |

## 阅读路径

| 你要做什么 | 先读 |
|---|---|
| 理解 aemeath 是什么、怎么划分 | [outline.md](outline.md) |
| 实现 / 改 Runtime 编排 | [runtime-design.md](runtime-design.md) + [outline.md](outline.md) §核心域 |
| 实现 / 改 TUI | [tui-design.md](tui-design.md) + [AGENTS.md](../../AGENTS.md) §触发表 `tui-cli` |
| 理解各层（contract/gateway/business/utils）的依赖方向 | [outline.md](outline.md) §COLA 工程分层 + [architecture-guards.md](architecture-guards.md) §5 |
| 新增 / 调整架构守卫 | [architecture-guards.md](architecture-guards.md) 全文 |
| Stop 钩子失败排查 | [architecture-guards.md](architecture-guards.md) §"守卫索引" + 相关小节 |
| 把巨型文件按层切分 | [file-split-plan.md](file-split-plan.md) |
| 准备做 server | [server-design.md](server-design.md) + [AGENTS.md](../../AGENTS.md) §开放决策 |

## 状态约定

- **已落地**：文档描述的状态机 / 规则 / 守卫**在当前 main 分支中已实现**；如发现与代码不符，按 [architecture-guards.md](architecture-guards.md) 流程处理（修文档或修代码，二选一并在 PR 中说明）。
- **演进中**：文档已定稿、配套迁移工作未完成；阅读时同时关注对应 GitHub Issue 进展。
- **草案**：文档在 `docs/snapshot/` 留有快照但代码未实现；落地时 **MUST** 同步在 [AGENTS.md](../../AGENTS.md) 触发表追加对应行。

## 维护规则

- 文档与代码冲突时，**以代码为准**——但**MUST** 在同一 PR 中同步更新文档。
- 任何对本目录的修改，**MUST** 走 worktree + PR（参见 [AGENTS.md](../../AGENTS.md) §Git 工作流）。
- 新增 / 撤销文档时，**MUST** 更新本 README 索引表与状态列。
