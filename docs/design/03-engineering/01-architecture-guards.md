# 架构守卫与白名单

> 对应实现：`.agents/architecture-guard-registry.json`（数据真相源）+ `tools/xtask/src/guards*.rs`（引擎）+ `.agents/hooks/check-architecture-guards.sh`（薄壳编排）。
>
> registry 是唯一调度源：规则即数据，引擎自动发现执行。文档与实现不一致时，**以 registry 与引擎代码为准**——它们是运行时真相源；本文档跟随实现迁移。

## 概述

守卫体系为「xtask guard 引擎 + registry 数据驱动 + 薄壳编排」三层形态：

- **registry**（`.agents/architecture-guard-registry.json`）：全部规则的数据真相源，含 `rules`（断言器数据行）、`construction_symbols`（跨 BC 构造白名单）、`entries`（例外/豁免登记）、`retired_symbols`（退役符号区）与 `budgets`（预算）。
- **引擎**（`tools/xtask/src/guards*.rs`）：`xtask guard [--fast|--full|--rule <id>]` 单一入口；启动时对 registry 做 schema 级自检（fail-closed），随后按档位筛选规则，以「文件 × 规则」循环执行（每文件文本与 use 索引经 `FileContext` 懒加载共享，至多一次读取/解析）。
- **薄壳**（`.agents/hooks/check-architecture-guards.sh`）：hook 入口适配层——先跑引擎，再执行少量尚未引擎化的 legacy 检查（见「薄壳 legacy 段」）。

拦截可靠性按优先级分三类机制：**编译期事实**（可见性收窄/私有构造器，违规编译不过）→ **引擎结构断言**（use 索引/导出白名单/依赖矩阵）→ **cargo test 行为断言**（进程/日志等运行时契约）。正则文本扫描只作为前两者的补充，且豁免清单全部由 registry 数据承载。

## 执行链路

```
┌─────────────────────────────────────────────────────────────┐
│ PreToolUse（Edit/Write，流程防护）                           │
│   └─ reject-main-edit.sh                                    │
│                                                              │
│ Stop（任务结束，快速反馈）                                   │
│   └─ check-agent-stop.sh                                    │
│       └─ check-architecture-guards.sh --fast                │
│            ├─ xtask guard --fast（结构类规则）               │
│            └─ legacy fast 段（少量独立脚本，并发）           │
│                                                              │
│ Git pre-push（完整合入前门禁）                               │
│   ├─ check-architecture-guards.sh --full                    │
│   │    ├─ xtask guard --full（全部规则 + 构造白名单）        │
│   │    └─ legacy full 段（registry 对账 / 行为测试 / 契约）  │
│   ├─ xtask test-runner（逐包 cargo test 门禁）               │
│   └─ clean-worktree-targets.sh --current --yes              │
└─────────────────────────────────────────────────────────────┘
```

`--fast` 档只跑结构类规则（`forbidden_segments` / `facade_whitelist` / `layer_order` / `layout` / `dependency_matrix`），秒级完成；`--full` 档跑全部规则与 `construction_symbols` 文本扫描。档位归属由每条规则的 `profile` 字段决定，文本扫描类规则 **NEVER** 进入 fast 档。

## registry 数据区

| 数据区 | 职责 | 关键字段 |
|---|---|---|
| `rules` | 断言器数据行（规则 = 数据） | `id` / `assertion` / `scope` / 断言器参数 / `reason` / `profile` / `exclusion_baseline` |
| `construction_symbols` | 跨 BC 构造点白名单（adapter/wire 两类） | `symbol` / `owner_crate` / `allowed_paths` / `guard` / `reason` |
| `entries` | 例外与豁免登记（migration_exception / scope_exclusion 等分类） | `classification` / `mechanism_type` / `owner` / `exit_condition` / `status` |
| `retired_symbols` | 已物理删除符号的登记（复活归 review，不设机械拦截） | `symbol` / `retired_by` / `reason` |
| `budgets` | 仓库与模块级 migration debt 预算 | `repository_migration_debt` / `modules` |

规则 id 命名：`{断言器族}.{域}.{关注点}`（如 `pattern.tui.render-output-purity`、`facade.project.root-exports`）。规则 id 一经分配 **NEVER** 复用于不同语义；退役规则保留 id 记录于修改历史。

## 断言器目录

引擎内置 9 种断言器（`tools/xtask/src/guards_rules.rs`），新增约束优先复用现有断言器加数据行：

| 断言器 | 语义 | 数据形态 | 档位 |
|---|---|---|---|
| `forbidden_segments` | use 路径禁穿透段（如跨 crate 禁 `feature::domain::`） | `forbidden_segments` + `allow_prefixes` | fast |
| `facade_whitelist` | crate 根 `pub use` 导出符号白名单（窄 façade） | `allowed_symbols` | fast |
| `layer_order` | 同 crate Hexagonal 层内依赖方向（`crate::` 路径段比对） | `layer_order` 层序数组 | fast |
| `layout` | 目录/顶层文件布局白名单 | `allowed_entries` | fast |
| `dependency_matrix` | workspace path 依赖边矩阵（cargo metadata） | `business_allow` | fast |
| `pattern_exclusion` | 禁用文本模式（子串）+ 豁免清单 + 行级 allow marker | `forbidden_patterns` / `exclusions` / `allow_marker` | full |
| `construction_whitelist` | 构造符号出现点白名单（由 `construction_symbols` 合成） | `symbol` + `allowed_paths` | full |
| `forbidden_file_names` | 禁文件名（如 `mod.rs`） | `forbidden_file_names` | full |
| `line_budget` | 单文件行数预算（职责不回缩锁） | `max_lines` 等 | full |

代表性规则族（完整清单以 registry 为准）：

- **跨 crate 边界**：`use.features.no-internal-segment-penetration`（内部层禁穿透）、`use.share.adapter-single-root`（adapter 唯一根）、`dependency.workspace-matrix`（依赖矩阵）。
- **窄 façade**：`facade.<crate>.root-exports` 系列（逐 crate 导出白名单）。
- **层序与布局**：`layer.<crate>.hexagonal-order`、`layout.<crate>.hexagonal-top-level` 系列。
- **构造与装配**：`construction_symbols` 数据区（owner crate 内部放行，越界构造与未登记跨 BC wire fail-closed）。
- **行为禁模式**：`pattern.config.app-service-no-direct-io`、`pattern.tui.model-update-no-direct-effects`、`pattern.logging.no-env-read`、`pattern.all.no-inline-test-modules`（`exclusion_baseline` 只降不升）等。

## 引擎能力边界（accepted gaps）

引擎的 use 分析是**单文件语法级索引**（syn 展开 use 树 + 路径段文本匹配），不是跨 crate 定义归属解析。以下绕过形态引擎抓不到，由 code review 与编译期类型化承担：

- 经 façade re-export 洗白内部符号（不解析目标 crate 的 re-export 链）；
- glob import 洗路径（`use share::*` 后引用内部模块）；
- use 别名洗路径；
- 表达式内全限定路径（无 use 语句）。

真正需要结构事实的约束 **MUST** 优先类型化（可见性收窄、私有构造器），使违规编译不过；引擎断言是辅助防线，**NEVER** 用文本扫描替代可类型化的约束。

## 编译期事实（类型化承接）

一批原脚本守卫已由代码设计收口为编译期事实（违规编译不过），不再有任何运行期检查。典型形态：

- 构造器收窄 `pub(crate)`（crate 外构造报 E0624）：`ConfigAppService`、各类 Store/Adapter；
- 字段私有化 + 只读访问器（装饰/改写编译期不可达）；
- 模块树位置表达归属（内部层 `mod` 无 `pub`，crate 外路径不可达）。

此类约束的复活决策归 review 与设计文档，**NEVER** 为其恢复文本黑名单脚本。

## cargo test 行为断言

运行时行为契约由 cargo test 承接（非引擎规则），挂薄壳 full 档：

- `cargo test -p logging routing_guard`：log target 路由与 TargetCatalog 一致性；
- `check-gate-layering-tests.sh`：gate 分层契约（Stop 只跑 `--fast`、pre-push 顺序、fail-fast 不清缓存）；
- 引擎 fixture 测试：`tools/xtask/tests/guard_cli.rs`（exit 0/2 语义）与 `guard_profile.rs`（档位打标策略）等。

## 薄壳 legacy 段

`.agents/hooks/` 当前文件与归宿：

| 文件 | 角色 | 归宿 |
|---|---|---|
| `check-architecture-guards.sh` | 薄壳编排（引擎入口 + legacy 段） | 长期保留（hook 适配层） |
| `check-agent-stop.sh` | Stop hook 入口（转发 `--fast`） | 长期保留 |
| `reject-main-edit.sh`(+tests) | PreToolUse 流程防护（强制 worktree 开发） | 长期保留（非静态结构事实） |
| `check-gate-layering-tests.sh` | gate 分层契约回归 | 保留（进程级契约测试） |
| `check-noninteractive-child-session.sh`(+tests) | 子进程 session 隔离（计数配比半段） | 待计数断言器批次退役 |
| `check-projection-naming.sh` | 命名守卫（标识符级正则） | 待 regex/标识符断言器批次退役 |
| `check-tui-unsafe-text-ops.sh` | 切片区间正则残段 | 待 regex 断言器批次退役（子串两模式已由 `pattern.all.no-unsafe-text-slicing` 承接） |

`xtask guard-registry check` 与 `xtask sdk-wire-schema check`、`xtask source-guard` 为 full 档内联调用，不再保留独立壳脚本。

## 元守卫（registry 对账）

`xtask guard-registry check`（薄壳 full 档执行）校验 registry 自身一致性：

- schema：必填字段、`classification` / `mechanism_type` 枚举、id 格式与唯一性、`status: active`；
- 预算：仓库与模块 migration debt 不超 `budgets`；
- 豁免基线：`exclusion_baseline` 只降不升（豁免增长必须先下调基线）；
- 引用对账：`entries` 的 `guard` 字段指向的脚本必须存在且含 `guard-registry:<id>` 标记；
- 新鲜度：entry scope 路径必须仍存在（stale 检测）；
- 文档对账：本文档与 `AGENTS.md` 引用的规则 id 必须存在于 registry（见「维护说明」）。

引擎启动自检（每次 `xtask guard` 运行）对 registry 做 schema 级校验，非法即 fail-closed。

## Git hooks（非架构守卫）

### pre-commit

`.cargo/hooks/pre-commit`（`core.hooksPath=.cargo/hooks`）：提交前轻量检查；详见该脚本头部注释。

### pre-push

- **位置**：`.cargo/hooks/pre-push`。
- **行为**：先 `check-architecture-guards.sh --full`，成功后 `xtask test-runner`，最后清理当前 worktree 构建缓存；任一步失败立即阻止 push 且 **NEVER** 清理缓存。
- **绕过**：仅使用 Git 原生 `--no-verify`；PR Test plan 必须披露并手工补跑两个完整入口。
- **已知边界**：worktree push 时 hooks 按主工作区版本执行（`core.hooksPath` 相对主仓库解析），删除 hook 脚本类的 PR 需在 Test plan 披露并手工补跑。

## 附：钩子体系（非架构守卫）

### reject-main-edit.sh（PreToolUse）

- **触发**：`PreToolUse` 钩子，`Edit` / `Write` 工具。
- **行为**：仅对 `Edit` / `Write` 生效；项目外文件放行；worktree 内放行；主工作区直接修改输出错误并 exit 2 阻断。
- **设计意图**：强制根指令的 Git 工作流——所有代码 / 文档 / 配置修改都在独立 git worktree 中执行。

### check-agent-stop.sh（Stop）

- **触发**：`.agents/aemeath.json` 的 `Stop` 钩子。
- **行为**：只转发到 `check-architecture-guards.sh --fast`（引擎结构类规则 + legacy fast 段，秒级）。
- **设计意图**：会话结束时的即时架构反馈，重检查（full 档、逐包测试）收敛到 pre-push。

### xtask test-runner（pre-push）

- **触发**：`.cargo/hooks/pre-push` 执行 `cargo run --quiet -p xtask -- test-runner`，仅在完整架构守卫通过后执行。实现：`tools/xtask/src/test_runner.rs`。
- **行为**：
  1. 清除 Git Hook 注入的 repository-local 环境变量，避免 `GIT_DIR` / `GIT_WORK_TREE` 等污染 Cargo 测试中的临时仓库；
  2. 设置 `CARGO_TARGET_DIR=target/hook-tests`（未显式指定时；隔离各 checkout 的 cargo 元数据）；
  3. 对包矩阵顺序跑 `cargo test`（默认 `--lib`；`composition` 跑 `--tests`，`cli` 跑 `--bin aemeath`）；
  4. 每包默认最多 180 秒（`AEMEATH_UNIT_TEST_TIMEOUT_SECS` 可调）；超时经独立进程组 TERM→KILL 收割并返回 124；
  5. 任一包失败 fail-fast，exit code 原样传播；
  6. 包日志写入 `<CARGO_TARGET_DIR>/hook-logs/<package>.log`，失败时打印错误摘要（前 40 行）。
- **回归测试**：`tools/xtask/tests/test_runner.rs`（fake cargo PATH stub：超时收割、exit 码传播、git env 净化、包矩阵断言）。

## 维护说明

- **新增约束**：优先判断能否类型化（编译期事实）；否则在 registry `rules` 加一行数据（复用现有断言器），引擎自动发现执行。确需新断言器时在 `guards_rules.rs` 实现并配正反例单测。**NEVER** 新增 `.agents/hooks/check-*.sh` 脚本。
- **新增豁免**：写入规则的 `exclusions` 并同步下调 `exclusion_baseline`（基线是收缩契约，只降不升）；裸豁免会被元守卫拒绝。
- **退役规则**：规则 id 保留在修改历史，**NEVER** 复用；防复活需求登记 `retired_symbols`（纯数据，复活归 review），不设文本黑名单。
- **文档对账**：本文档与 `AGENTS.md` 引用规则 id 时使用反引号完整 id（如 `pattern.all.no-inline-test-modules`），元守卫校验引用必须存在于 registry。
- **冲突解决**：本文档与 registry/引擎不一致时，**以 registry 与引擎代码为准**——它们是运行时真相源；本文档跟随实现迁移。

## 相关文档

- [架构守卫收敛判据清单（执行记录）](../../snapshot/specs/guard-consolidation-matrix.md)：90 脚本 → 引擎的逐条消解判据与批次进度。
- [依赖规则](../01-system/05-dependency-rules.md)：R8 同 crate Hexagonal 层内依赖方向（`layer_order` 断言器的规则语义来源）。
- [代码组织](../01-system/06-code-organization.md)：Hexagonal crate 内部默认与 façade 约定。
- [Migration Governance](03-migration-governance.md)：迁移例外、责任与退出条件。
- [测试与覆盖](04-testing-and-coverage.md)：测试分层与守卫在其中的位置。

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-09-29 | 终态重写：守卫体系收敛为「xtask guard 引擎 + registry 数据驱动 + 薄壳编排」后的全量改写；原 90 个脚本的逐条归宿见判据清单文档；执行链路、断言器目录、数据区、能力边界与维护流程按终态重述。 |
| 更早 | 脚本时代的逐守卫演进记录已随重写归档（历史版本见 git 记录）。 |
