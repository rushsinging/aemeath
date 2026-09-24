# 守卫收敛判据清单（Guard Consolidation Matrix）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1671

本清单是守卫体系收敛（shell 脚本层 → xtask 引擎 + registry 数据驱动）的第一批交付物：`.agents/hooks/` 全部 90 个文件的逐条消解判据与迁移动作——63 个业务主脚本（含 1 个 `.py`）、1 个编排器、25 个真自测脚本、以及 2 个文件名以 `-tests.sh` 结尾但实为主脚本的特例（`check-no-inline-tests.sh`、`check-unit-tests.sh`，分别归入 F/A 组）。本文档是迁移执行的唯一依据，**MUST** 先评审合入，再开始任何脚本删除或引擎迁移；每条记录 **NEVER** 允许「未判定即删除」。

## 1. 判据定义

每条规则按四选一判据消解（一条复合脚本允许拆分后分别归组）：

| 判据 | 归宿 | 判定标准 |
|---|---|---|
| ① 类型化 | 改代码设计使违规**编译不过**，规则退役 | 约束可用可见性收窄、私有构造器、sealed trait、模块树位置表达 |
| ② 测试化 | 转 `cargo test`（测试分层核验承接） | 约束是运行时行为语义而非结构事实 |
| ③ 数据化 | 进 registry 数据行，由引擎断言器执行 | 跨 crate / 跨目录结构事实：use 路径段、导出白名单、层序、目录布局、计数 |
| ④ 退役 | 删除守卫，复活归 review | 防复活黑名单（改名即绕过、防御价值趋零）、与既有规则重复、被编译期事实覆盖 |

**通用原则**：

- 所有脚本内嵌的「退役符号不得回归」子项，统一切出到 registry 的 `retired_symbols` 数据区后随宿主脚本删除；**NEVER** 为防复活名单保留独立脚本。
- 迁移等价性以现有自测脚本的故意违规用例为验收基准（fixture 化后 `cargo test`）。
- 检验标准：若某条规则在 xtask 内仍需一次性命令式代码，说明选错层，**MUST** 回到 ①/②/④ 重新判定。

## 2. 汇总

| 组 | 数量 | 处置 |
|---|---|---|
| A. 转发壳与编排器 | 7 | → 1 个薄壳 + `xtask guard` 入口 |
| B. 自测脚本（真 `X-tests.sh`） | 25 | → cargo 单测 + fixture |
| C. ④ 直接退役 | 4 | → 删除（含 registry entry 同步 retire） |
| D. ① 类型化（含混合主判①） | 15（14 条规则，`config-adapter` sh+py 计 2 文件；1 条已执行，`block-nesting` 判据修正移 F） | → 构造器/可见性收窄，规则随编译强制退役 |
| E. ② 测试化 | 8 | → cargo test |
| F. ③ 数据化 | 30（F-30 为 D 组判据修正移入） | → registry 数据 + 引擎断言器 |
| G. 保留独立 hook | 1 | 流程防护，不进 xtask |
| 编排器内嵌规则函数 | 1 | 归组 D（见 3.1） |

合计 90 文件。配套事实：8 个主脚本在 registry 无 entry（详见各组「补录」标记）；`check-cross-bc-construction-registry.sh` 已是「registry 数据 + fail-closed 引擎」目标形态，作为引擎迁移样板。

### 2.1 当前形态与目标形态对比

| 维度 | 当前（混杂形态） | 目标（终态） | 效果 |
|---|---|---|---|
| 入口 | 编排器 + 多个转发壳各自被 hook 调用 | 1 个薄壳 → `xtask guard --fast/--full`（+ `reject-main-edit` 独立流程防护） | hook 链单一化 |
| 规则载体 | ~1 万行 bash/perl/python 命令式检查代码（含 perl 单行转义地狱） | registry 数据行（白名单/路径段/层序矩阵/豁免清单）+ 4 个 Rust 断言器 | 600K 脚本 → 引擎一次实现 + 纯数据 |
| 新增约束成本 | 新脚本 + 配套自测脚本 + 编排器登记 + registry entry，四处同步 | registry 加一行数据，引擎自动发现执行 | 触点 4 → 1，嘈杂增长源消除 |
| 规则自测 | 25 个「测脚本的脚本」（fixture 重放 shell 逻辑，双倍维护） | 规则函数 cargo 单测 + fixture，类型安全 | 同一语义单处维护 |
| 拦截可靠性 | 正则匹配源码文本：符号改名即绕过，误报靠 inline-exclusions 层层打补丁 | ① 编译期事实（违规编译不过）+ ③ use 图/符号白名单结构断言 + ② 行为测试 | 从「模式猜测」升为「结构事实」 |
| 防复活名单 | 散落在各脚本的符号黑名单，彼此不知、维护分散 | `retired_symbols` 集中数据区，不设断言器、复活归 review | 黑名单退役（#1021 决策落地） |
| 违规输出 | 各脚本自由格式，定位与修复提示不一 | 统一 `rule_id + file:line + remediation` | 可诊断、可统计、可 CI 化 |
| 执行模型 | 编排器 shell 后台并发 + timeout 管理 | xtask 进程模型 + fast/full profile | 调度逻辑可测 |
| 文档一致性 | AGENTS.md 记「17 个 guard」实际 90（漂移 5 倍）；白名单硬编码在脚本内 | registry 单一真相源，元守卫对账文档/registry/引擎三方一致 | 漂移检测机制化 |
| 语言栈 | bash + perl + python 三种混用 | Rust 单一 + JSON 数据 | 维护技能面收窄 |

量化口径：入口文件 90 → 2；命令式守卫代码 ~10k 行 → 引擎（一次实现）+ 数据行；新增约束触点 4 处 → 1 处。

## 2.2 执行链路：当前与目标

### 当前（3 个触发点，实测自 `.agents/aemeath.json` 与 `.cargo/hooks/pre-push`）

```
Agent PreToolUse (Edit/Write)                    Agent Stop (timeout 150s)
        │                                                │
        ▼                                                ▼
reject-main-edit.sh                            check-agent-stop.sh
（流程防护，保留）                                       │
                                                        ▼
git pre-push (.cargo/hooks) ──────────► check-architecture-guards.sh --fast
        │                                        │（18KB 编排器）
        ▼                                        ├─ 内嵌 perl 函数（如 run_tui_single_source_structure_guard）
check-architecture-guards.sh --full             ├─ guarded()/run_guard() 并发调度 + timeout 管理
+ check-unit-tests.sh                           ▼
+ clean-worktree-targets.sh              ~63 个主脚本各自执行
                                         ├─ grep/perl/python 自由格式输出
                                         ├─ 4 个壳再转发 xtask 子命令
                                         │   （source-guard / sdk-wire-schema / guard-registry check）
                                         └─ registry 仅作元数据被对账，NOT 调度源
```

缺陷：编排器与 63 脚本各自为政；规则 = 代码；registry 不驱动执行；输出格式不一；新增约束需 4 处同步。

### 目标

```
PreToolUse (Edit/Write)              Agent Stop                    git pre-push
        │                                │                             │
        ▼                                ▼                             ▼
reject-main-edit.sh                 薄壳（唯一保留的 .sh）         薄壳 + xtask test-runner
（流程防护，不变）                        │                             │
                                         └──────────┬──────────────────┘
                                                    ▼
                                        xtask guard --fast | --full | --rule <id>
                                                    │
                                        ① registry 启动自检（原 guard-registry check）
                                                    ▼
                                        引擎：cargo metadata + syn 解析 use 树
                                                    │
                                    ┌───────────┬───┴────────┬───────────┐
                                    ▼           ▼            ▼           ▼
                            forbidden_     facade_       layer_order/   pattern_
                            segments       whitelist     layout         exclusion
                                    └───── registry 数据行（唯一规则来源，含豁免）
                                                    ▼
                                        统一输出：rule_id + file:line + remediation
```

两条旁路（**NEVER** 进入 guard 链路）：

- **① 类型化（D 组 15 条）**：`cargo build` 即守卫，违规编译不过；
- **② 测试化（E 组 8 项 + 25 自测）**：`cargo test` 即守卫。

执行映射：引擎落地（建链路）→ 退役壳收敛 + 数据化迁移（换链路、删旧）→ 类型化 + 测试化（旁路化）。

## 3. 逐条清单

### 3.1 A 组：转发壳与编排器（7 文件）

| 文件 | registry | 内容 | 判据 | 动作 |
|---|---|---|---|---|
| check-architecture-guards.sh | —（编排器） | 内嵌 TUI 结构守卫函数 + `guarded`/`run_guard`/`wait_for_fast_guards` 超时并发调度 | 拆分 | 调度层 → `xtask guard [--fast\|--rule <id>]`；内嵌函数 `run_tui_single_source_structure_guard` → D-13；`report_matches` helper → 引擎统一断言 API |
| check-agent-stop.sh | 无 | Stop hook 转发编排器 `--fast` | A | 并入薄壳（Stop 场景 = `xtask guard --fast`） |
| check-production-reachability.sh | 无 | 转发 `xtask source-guard` | A | 统一 guard 入口调用，壳删除 |
| check-sdk-wire-schema.sh | 无 | 转发 `xtask sdk-wire-schema check` | A | 同上 |
| check-guard-registry.sh | 无 | 转发 `xtask guard-registry check` | A | 成为引擎启动时 registry 自检步骤 |
| check-unit-tests.sh | 无 | 逐包 cargo test 执行编排（超时/摘要） | A→② | 并入 xtask test-runner，包清单进配置 |
| check-log-target-prefix.sh | scope.logging.production-test-sources | 转发 logging crate 内 routing_guard 测试 | A→② | 并入薄壳；target 前缀规范可另立 ③ 规则 |

### 3.2 B 组：自测脚本（25 文件，统一处置 → ②）

check-command-catalog-boundary-tests、check-composition-construction-ownership-tests、check-config-store-ownership-tests、check-cost-tracker-retirement-tests、check-crate-api-boundary-tests、check-cross-bc-construction-registry-tests、check-gate-layering-tests、check-hexagonal-layer-purity-tests、check-hook-target-facade-tests、check-no-inline-tests-tests、check-no-mod-rs-tests、check-noninteractive-child-session-tests、check-provider-window-single-owner-tests、check-runtime-capability-assembly-ownership-tests、check-runtime-event-naming-tests、check-runtime-hook-assembly-ownership-tests、check-runtime-large-file-responsibilities-tests、check-runtime-tool-assembly-ownership-tests、check-session-management-ownership-tests、check-session-project-scope-tests、check-shared-run-loop-tests、check-tool-catalog-execution-boundary-tests、check-tui-retained-output-view-tests、check-unit-tests-tests、reject-main-edit-tests。

> 文件名以 `-tests.sh` 结尾但不属于本组的两个特例：`check-no-inline-tests.sh`（F-28）、`check-unit-tests.sh`（A 组）。`check-gate-layering-tests` 无同名主脚本，判定其引用的 gate 分层约束归属后随最邻近规则一并迁移。

动作：宿主规则迁入 xtask 后，其 fixture（故意违规用例）转为规则函数的 cargo 单测；「测脚本的脚本」模式随宿主消亡。

### 3.3 C 组：④ 直接退役（4 文件）

| 文件 | registry | 内容 | 理由 | 动作 |
|---|---|---|---|---|
| check-tui-output-legacy-guards.sh | false-positive.tui.running-indicator-condition | 已退役并行 Run 生命周期符号防复活 + 条件豁免 | 纯黑名单 + false_positive_suppression，改名即绕过 | 删除脚本与豁免 entry；复活归 review |
| check-cost-tracker-retirement.sh | policy.audit.cost-retirement | 已退役 Cost/Pricing 符号与 cost_history 防复活 | 同上；退役代码已物理删除 | 删除脚本，entry 状态 retire |
| check-hook-target-facade.sh | policy.hook.target-facade | 禁恢复已删除的 hook::api 路径 | 引用已删除路径本就编译失败 | 删除脚本，entry retire |
| check-share-no-upstream-deps.sh | 无 | share/Cargo.toml 不得 path 依赖上游 crate | 与 cargo 依赖矩阵（F-4）share 行完全重复 | 删除，去重并入矩阵 |

### 3.4 D 组：① 类型化（14 文件）

| 文件 | registry | 内容 | 类型化动作 |
|---|---|---|---|
| check-runtime-capability-assembly-ownership.sh | policy.runtime.capability-assembly | RuntimeContext/Token 唯一构造点等 21 组混检 | 构造器私有化（composition 唯一）；导出白名单子项→F；退役符号子项→`retired_symbols` |
| check-runtime-hook-assembly-ownership.sh ✅（已执行） | policy.runtime.hook-assembly.composition-ownership（entry 已删） | Hook dispatcher 仅 composition 构造 | 已完成（判据修正①→③）：跨 crate 无排除可见性，落 pattern.runtime.no-hook-dispatcher-construction 引擎规则（hook:: 前缀锚定 + context_factory.rs 单文件豁免登记）；旧守卫截断式剥离的过剥盲区由引擎精确块剥离修复 |
| check-runtime-tool-assembly-ownership.sh ✅（已执行） | policy.runtime.tool-assembly.composition-ownership（entry 已删） | Tool/Skill/Registry 仅 composition 装配 | 已完成：ActiveRunRegistry 摘 derive(Default)（仅 cfg(test)）+ wire_active_run_registry 工厂唯一生产构造（capability façade/construction_symbols 双白名单登记）；负向禁式落 pattern.runtime.no-tool-self-assembly；正向装配断言由必填字段类型承接（④）；元守卫映射表与自测壳同步 |
| check-runtime-activity-observation.sh ✅（已执行） | policy.runtime.activity-observation（entry 已删） | ActivityObservation 唯一构造/变更点 + legacy 符号 | 已完成：构造限点落 pattern.runtime.activity-construction-single-owner（coordinator/model 双文件豁免）；TUI 变更入口已 #[cfg(test)] 方法级编译锁定（④）；legacy/hook 平行黑名单符号已物理删除（④），复活归 review |
| check-provider-construction-ownership.sh ✅（已执行） | 无（随退役免录） | provider 构造符号仅 composition 可引用 | 已完成（判据修正①→③）：跨 crate 单点授权编译期不可表达，落 pattern.features.no-provider-composition-penetration（features scope + provider 自身豁免；构造符号已从 crate-root 退役，根穿透编译期拦） |
| check-provider-invocation-scope.sh ✅（已执行） | scope.provider.invocation-tests（entry 已删） | invocation_stream 强制 `&InvocationScope`、禁可变状态 | 已完成：签名断言由 trait 类型系统锁定（④）；可变状态禁式落 pattern.provider.no-mutable-invocation-state 与 pattern.runtime.no-shared-client-mutation 两规则 |
| check-provider-window-single-owner.sh ✅（已执行） | 无 | ContextWindow 映射唯一 owner，禁再装饰 messages_for_api | 已完成：InvocationContext.messages_for_api 字段私有化 + 只读访问器（装饰编译期不可达，mapper 唯一产出）；reminder 标签渲染半边按 #1021 决策归 ④（hook_notice 为 hook 系统合法通道，typed InvocationReminder 已建立）；正向接线断言（coordinator 字面量）④ 退役归测试 |
| check-config-reader-injection.sh ✅（已执行） | scope.config.reader-tests-only（entry 已删） | ConfigAppService 构造限定 config/composition | 已完成：`new`/`with_global_path`/`with_env_source`/`with_native_store` 收窄 `pub(crate)`，crate 外违规编译报 E0624；类型渗漏半边（TUI/CLI 禁持 reader 类型）按本表原计划归 pattern_exclusion 数据规则 |
| check-config-store-ownership.sh ✅（已执行） | policy.config.override-store.composition-ownership（entry 已删） | BlobAdapter/NativeConfigStore 唯一工厂构造 | 已完成：`NativeConfigStore::new` 收窄 pub(crate) + crate 根 `native_override_store` 工厂（E0624 证据）；config 禁 blob 半边落 registry 首条引擎规则 `pattern.config.no-blob-construction`（故意违规 exit 2 实证）；same-body 组织约束按④退役归 review |
| check-composition-construction-ownership.sh ✅（已执行，与 session-management 条合并交付） | policy.composition.cross-bc-construction-ownership（entry 已删） | BC 内禁构造他方 adapter | 已完成：三文件禁式已被 pattern.runtime/config/context 三条引擎规则覆盖（scope 扩至 crate src）；leaf 映射表随最后 leaf 退役清空；脚本与自测壳删除 |
| check-context-architecture.sh | 无（主体随①消失；R3/R8/R12 黑名单进 `retired_symbols`） | R1–R12：上下文类型禁字段、能力调用限定点 | 字段与可见性改型 |
| check-unified-authorization.sh | 无 | 授权统一（混合①②④） | Allow 携带上下文→类型化；patch 顺序→cargo test；legacy 退役项→`retired_symbols` |
| check-config-adapter-boundary.sh + .py（2 文件）✅（已执行） | 无 | config application 禁直读 fs/JSON + stub 残留 | 已完成：注入式 #1654 已落地，防回退落两条引擎规则（app-service-no-direct-io / adapters-no-stubs），sh+py+编排器行删除；判据修正记录：同 crate 跨模块 API 禁止无法编译期表达，①不可行 |
| 编排器内嵌 run_tui_single_source_structure_guard | scope.tui.arch.inline-exclusions | retired widget adapter 仅 cfg(test)、render 禁镜像存储/生产读写 API | retired adapter 文件移出生产模块树（或删除）；镜像字段可见性收窄；CompactProgress 黑名单→`retired_symbols` |
| check-session-management-ownership.sh | policy.session-management.composition-ownership | Session backing 仅 composition 构造 | 构造器私有化 + 注入 port |

（注：D 组 14 条规则记录、15 个文件——`check-config-adapter-boundary` 的 `.sh` 与 `.py` 为一对，合计时按 2 文件计；编排器内嵌函数归 D 但文件本体在 A 组；`check-config-reader-injection.sh` 已执行完毕。`check-tui-block-nesting.sh` 经执行核验**判据修正**：Rust 可见性无法表达「对特定子模块隐藏」（blocks 与 gutter 同属 output 祖先链，任何收窄都无法单独屏蔽 blocks），而实际调用面为 document_renderer 与 primitives/markdown 两处，强行类型化需改渲染逻辑——降级为 F 组 pattern_exclusion（F-30），scope=blocks 目录、禁模式=`apply_gutter`、无豁免。）

### 3.5 E 组：② 测试化（8 文件）

| 文件 | registry | 内容 | 动作 |
|---|---|---|---|
| check-runtime-event-naming.sh | policy.runtime.event-naming | 事件枚举与 baseline 索引对账、命名后缀受控 | baseline variants 数据进 registry；对账转 cargo test |
| check-provider-http-attempt.sh | scope.provider.error-log-self-reference | HTTP 发送唯一经 http_attempt、错误体读取限 executor | cargo test 行为断言 |
| check-provider-retry-ownership.sh | 无 | 拉流段禁重试循环/backoff/回退 | cargo test 断言重试与回退语义 |
| check-provider-usage-capability.sh | 无 | RawUsage 缺失值禁抹零、clamp 唯一 | cargo test 解析断言 |
| check-task-state-pipeline.sh | 无 | mutation 禁从 prose 推断、committed 元数据必携 | cargo test；退役符号→`retired_symbols` |
| check-noninteractive-child-session.sh | policy.process.noninteractive-child-session | 子进程必须脱离控制终端 | cargo test 无 TTY 断言；session owner 事实→F |
| check-session-project-scope.sh | policy.context.session-project-scope | 项目隔离 API 强制 | cargo test 跨项目不可见断言 |
| check-tui-retained-output-view.sh | 无 | 生产刷新走 RetainedOutputView | 正向不变量→cargo test；防复活段→`retired_symbols` |

### 3.6 F 组：③ 数据化（29 文件）

| # | 文件 | registry | 数据化内容 |
|---|---|---|---|
| F-1 | check-cross-bc-construction-registry.sh | policy.cross-bc.construction-registry + construction.*×24 | **引擎迁移样板**：已 registry 驱动、fail-closed，逻辑直接成为引擎断言器 |
| F-2 | check-crate-api-boundary.sh | policy.{task,hook,context}.crate-root-facade | INTERNAL_SEGMENTS 段黑名单 + ROOT_REEXPORT_ALLOW 等导出白名单全量下沉 registry |
| F-3 | check-hexagonal-layer-purity.sh | policy.hexagonal.current-layer-matrix 等 | 层目录矩阵 + 层序断言；RETIRED_COLA_LAYERS→`retired_symbols`；update 例外保留 migration_exception |
| F-4 | check-cargo-dependency-graph.sh | policy.cargo.capability-dependency-matrix | business_allow 依赖矩阵进 registry，引擎走 cargo metadata |
| F-5 | check-forbidden-imports.sh | policy.composition.unique-adapter-root | 禁段 + 允许前缀规则 |
| F-6 | check-cli-thin-entry.sh | 无（补录） | cli 依赖白名单 + bootstrap 段规则 |
| F-7 | check-config-env-guard.sh | policy.config.business-env-owner | env 名单 × 允许路径白名单 |
| F-8 | check-composition-layout.sh | 无（补录） | 顶层文件白名单 + lib.rs 模块声明集合 |
| F-9 | check-share-minimal-kernel.sh | policy.shared.task-owner-boundary | 依赖白名单 + 禁用 API 名单；forbidden_modules→`retired_symbols` |
| F-10 | check-command-catalog-boundary.sh | scope.command.tests-and-owner-filter | 定义点 owner 白名单；builtin 恢复段→`retired_symbols` |
| F-11 | check-agent-client-trait-minimal.sh | 无（补录） | trait 方法白名单；Cancel 入口→`retired_symbols` |
| F-12 | check-task-persistence-capability.sh | policy.task.access-persistence-split | forbidden 符号名单；探针转单测 |
| F-13 | check-shared-run-loop.sh | scope.runtime.shared-loop-tests | run_loop 计数 + owner 接线清单；退役符号→`retired_symbols` |
| F-14 | check-runtime-large-file-responsibilities.sh | policy.runtime.large-file-responsibilities | 行数预算 + 必需文件清单；退役符号→`retired_symbols` |
| F-15 | check-provider-driver-acl.sh | 无 | 导出白名单 + 禁穿透段 |
| F-16 | check-tool-catalog-execution-boundary.sh | 无 | 路径段/导出/文件存在规则；legacy 符号段→`retired_symbols` |
| F-17 | check-tui-effect-boundary.sh | 无 | 目录 × 副作用 API 族黑名单 |
| F-18 | check-tui-model-view-boundaries.sh | 无 | 层边界 use 依赖；legacy 路径段→`retired_symbols` |
| F-19 | check-tui-render-isolation.sh | 无 | 目录依赖 + 禁用符号 + 豁免清单 |
| F-20 | check-tui-render-pure.sh | scope.tui.render-tests-and-display-bridge | use 路径段黑名单 |
| F-21 | check-tui-tea-purity.sh | scope.tui.tea-runtime-files 等 | 目录 × 副作用 API + 文件/行豁免数据 |
| F-22 | check-tui-toplevel-layout.sh | 无 | 顶层目录白名单；旧路径段→`retired_symbols` |
| F-23 | check-tui-unsafe-text-ops.sh | scope.tui.safe-text-owners 等 | 危险文本操作模式 + 多层豁免 |
| F-24 | check-run-control-boundary.sh | 无 | SDK DTO 禁用类型名单 + 必需 API 白名单 |
| F-25 | check-compact-continuation-checkpoint.sh | context.compact-continuation-checkpoint | 状态归属 + 字面量契约 |
| F-26 | check-logging-scope-context.sh | scope.logging.registered-process-statics 等 | 静态白名单 + 文件断言；legacy 段→`retired_symbols`；spawn/capture 断言可转 test |
| F-27 | check-logging-settings-injection.sh | scope.logging.settings-tests 等 | 导入白名单 + 唯一性计数 |
| F-28 | check-no-inline-tests.sh | 无（基线 .agents/inline-tests-baseline.json） | 测试文件布局断言 + 存量基线（随迁移收缩至删除） |
| F-29 | check-no-mod-rs.sh | 无 | `forbid: src/**/mod.rs` 声明式规则 |
| F-30 | check-tui-block-nesting.sh（自 D 组判据修正移入） | 无（补录） | blocks 目录禁 `apply_gutter` 模式（gutter 由 renderer 注入；类型化不可行：Rust 可见性无法对特定子模块隐藏） |

### 3.7 G 组：保留独立（1 文件）

| 文件 | 理由 |
|---|---|
| reject-main-edit.sh（+ 其 tests，见 B 组） | PreToolUse 流程防护（强制 worktree 开发），依赖 hook 运行时上下文，非静态结构事实，xtask 无法替代；保持独立薄 hook |

## 4. 引擎断言器映射（③ 的归宿）

F 组 29 项 + D/E 组切出的白名单子项，收敛为 4 个通用断言器：

| 断言器 | 覆盖 | 数据形态 |
|---|---|---|
| forbidden_segments | F-5/15/20、F-2 段黑名单、各「禁穿透路径段」子项 | `{scope, forbidden_segment, allow_prefix[]}` |
| facade_whitelist | F-2 导出白名单、F-11/24、各导出面子项 | `{crate, symbols[]}` |
| layer_order / layout | F-3/8/18/22/28/29、F-4 依赖矩阵 | 层序矩阵 + 目录白名单 + 依赖矩阵 |
| pattern_exclusion | F-17/19/21/23/26/27、F-7 | 目录/文件 × 禁用模式 × 豁免清单 |

`retired_symbols` 为纯数据区（不设断言器），供 review 对照，不再机械拦截。

## 5. 执行顺序

1. **PR 1**（本文档）：判据清单评审合入。
2. **PR 2**：xtask 引擎 + 断言器 + registry schema 扩展 + fixture 单测；新旧并存可切换，薄壳就位。
3. **PR 3+**：按 C→B(随宿主)→F→D→E 分批迁移退役，每批以 fixture 等价性验收；D 组涉及生产代码可见性改动，逐条独立 PR 评审。
4. **收尾**：`.agents/hooks/` 仅剩 1 薄壳 + reject-main-edit；AGENTS.md 触发表与 `docs/design/03-engineering/01-architecture-guards.md` 改写；父项完成定义同步。
