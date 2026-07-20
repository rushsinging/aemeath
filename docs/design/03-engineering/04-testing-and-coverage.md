# 测试架构与覆盖率治理

> 层级：03-engineering（横切工程关注点）
> 状态：Target（测试组织规范已落地，覆盖率/生产可达性/场景实现待后续 Issue）｜Milestone：v0.1.0｜对应 Issue：[#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006)、[#1013](https://github.com/rushsinging/aemeath/issues/1013)
> 本文定义 workspace 统一测试分层、目录组织、fixture/替身、覆盖率、生产可达性与 CI 门禁。Rust 代码变更的可执行约束以 [`specs/rust-coding.md`](../../../specs/rust-coding.md) 为准。

## 1. 目标与非目标

### 1.1 目标

1. 按验证责任组织测试，而不是按“每个公共函数固定若干用例”机械计数。
2. 让单元、模块协作、契约、场景和系统 smoke 测试互补，避免重复覆盖或跳层。
3. 用统一覆盖率信号观察 workspace、crate 和 changed lines 的变化。
4. 将“被测试执行”与“生产代码可达”分开治理，避免测试引用掩盖 dead code。
5. 让时间、ID、路径、环境和外部端口可注入，消除并行执行下的 flaky 测试。
6. 允许现有测试渐进迁移，**NEVER** 为目录统一一次性移动全仓测试。

### 1.2 非目标

- 覆盖率百分比不证明行为正确，也不替代 code review。
- 单元测试不承担真实终端、真实网络和发布资产验证。
- 测试数量不是质量指标；相同分支上的多个重复测试不增加有效覆盖。
- `cargo clippy --all-targets` 不能单独证明生产代码没有 dead code。

## 2. 六层测试模型

| 层级 | 名称 | 验证责任 | 典型入口 |
|---|---|---|---|
| L0 | 编译期约束 | 类型、trait、feature、架构依赖和 production reachability | compiler、clippy、architecture guards |
| L1 | 单元测试 | 值对象、纯函数、单条状态转换和局部不变量 | inline `mod tests`、同级 `*_tests.rs` |
| L2 | 模块协作测试 | 同一 crate 内 service、port、reducer、assembler 的协作 | `src/<owning-layer>/<module>/tests.rs` + `tests/` |
| L3 | 契约测试 | Published Language、Port/Adapter、序列化和兼容性 | crate 根 `tests/`、contract suite |
| L4 | 场景测试 | 跨多个内部层的用户或业务旅程 | `scenario_tests.rs` + `scenario_tests/` |
| L5 | 系统 smoke | 真进程、PTY、平台、发布资产和安装路径 | 独立 CI suite |

### 2.1 L0 编译期约束

L0 不执行行为断言，但负责阻止结构退化：

- 生产 target 独立编译和 lint；
- 全 target lint；
- 架构依赖守卫；
- feature/platform 编译矩阵；
- public API surface 审计；
- 测试专属入口泄漏守卫。

### 2.2 L1 单元测试

适用于：

- 值对象与 parser；
- 纯计算、格式化和错误分类；
- reducer 的一个 intent；
- 状态机的一条转换和不变量；
- 序列化字段的局部行为。

L1 **MUST** 快速、确定、无真实网络和用户目录 I/O。需要文件系统时使用每测试独立临时目录；需要时间时注入 clock。

### 2.3 L2 模块协作测试

适用于同一 crate 内多个对象共同完成的行为：

- Application Service + Fake Port；
- reducer + Model；
- Runtime loop 的单个阶段；
- ViewAssembler + Model/ViewState；
- Provider 请求构造 + 测试 server；
- Storage service + 临时目录。

L2 是白盒测试，可以访问 crate 私有模块，但不得穿越 crate 边界访问另一 crate 的内部实现。

### 2.4 L3 契约测试

适用于稳定边界：

- SDK Published Language 序列化兼容；
- Port 对所有 Adapter 的共同约束；
- Config 优先级；
- Storage schema 兼容；
- Tool schema；
- Runtime → SDK → TUI ACL 字段完整性。

同一契约的测试逻辑 **MUST** 定义一次，通过 factory/fixture 对多个 adapter 复用，**NEVER** 为每个实现复制同一断言集合。

### 2.5 L4 场景测试

L4 验证用户可感知或业务可验收旅程。TUI 的 crossterm → update → Effect → Runtime 回灌 → ViewAssembler → TestBackend → insta 属于 L4，详见 [../02-modules/tui/05-e2e-scenario-testing.md](../02-modules/tui/05-e2e-scenario-testing.md)。

L4 不替代 reducer、Buffer cell 和契约测试。跨层链路应在每个边界保留相邻测试，并由场景测试证明最终组合成立。

### 2.6 L5 系统 smoke

L5 只覆盖进程内 Harness 无法覆盖的职责：

- raw mode、alternate screen 和真实 `EventStream`；
- signal、panic 后终端恢复；
- CLI 进程启动和退出；
- 平台特定行为；
- release artifact 的安装和执行。

L5 数量应少而稳定，禁止把细粒度业务组合全部推到昂贵的系统测试。

## 3. 按行为类型选择覆盖策略

“每个公共函数正常/边界/错误各一例”只适合部分纯逻辑，不作为全仓统一规则。目标覆盖方式如下：

| 行为类型 | 必要覆盖证据 |
|---|---|
| 纯函数/parser | 有效等价类、关键边界、错误输入 |
| 状态机/reducer | 文档化的可达转换、非法转换、状态不变量 |
| Application Service | 成功、Port 失败、幂等/重试/并发语义（适用时） |
| Adapter | 共享契约、协议错误、兼容性和资源释放 |
| 序列化类型 | round-trip、缺省字段、旧格式兼容、未知字段策略 |
| TUI widget | 关键尺寸、Unicode、style/cell 和选择区域不变量 |
| TUI 场景 | 用户旅程、中间状态、Effect payload、最终 framebuffer |
| 一行委托/getter | 可由上层行为间接覆盖，不强制独立测试 |

新增或修改核心行为 **MUST** 有可追溯测试证据。测试证据可位于 L1～L5 的适当层，不要求每个生产文件都物理内嵌测试模块。

## 4. 目录组织

### 4.1 小型单元测试：源文件内联

```text
parser.rs
└─ #[cfg(test)] mod tests { ... }
```

适用于 fixture 简单、测试较少且生产文件不会因此失去可读性的模块。

### 4.2 大型单元测试：同级 `*_tests.rs`

```text
parser.rs
parser_tests.rs
```

生产文件只保留：

```text
#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
```

适用于单一生产模块的表驱动测试、复杂边界矩阵或较长测试集。测试模块统一命名为 `tests`，文件名表达被测对象。

### 4.3 模块协作测试：模块内 `tests/`

```text
src/<owning-layer>/<module>/
  service.rs
  reducer.rs
  tests.rs
  tests/
    submit.rs
    cancel.rs
    compact.rs
```

`tests.rs` 声明 `mod submit; mod cancel; mod compact;`。一个子文件对应一个稳定行为或用户故事。所有测试模块采用 Rust 2018+ 的同名文件与目录并存形状，**NEVER** 新增 `mod.rs`。不分层 crate 可省略 `<owning-layer>`；采用 Hexagonal Architecture 的 crate 则必须使用真实 owning layer，例如 Runtime 的 `domain/<module>/tests/`、`application/<module>/tests/`、`ports/<seam>/tests/` 或 `adapters/<adapter>/tests/`。`shared` 仅在存在真实共享内容时按需创建；测试目录 **NEVER** 引入第二套 VSA，也不得建立跨层万能测试层。

**NEVER** 使用 `include!("tests/*.rs")` 拼接测试文件。`include!` 共享隐式作用域，降低 IDE、诊断、模块归属和覆盖报告可读性；现存用法按相关模块变更渐进迁移。

### 4.4 契约测试：crate 根 `tests/`

```text
packages/sdk/tests/published_language_compat.rs
agent/features/provider/tests/provider_contract.rs
```

crate 根 integration test 只能通过公共 API 验证契约。若契约需要多个实现共享，应暴露最小 test factory，而不是扩大生产 API。

### 4.5 场景测试：专用模块

```text
apps/cli/src/tui/
  testing.rs
  testing/
    harness.rs
    effect_driver.rs
    fixture.rs
    virtual_clock.rs
  scenario_tests.rs
  scenario_tests/
    startup.rs
    chat.rs
    tool.rs
    ask_user.rs
    layout.rs
```

`testing.rs` 保存并声明 Harness、Fake、fixture 和虚拟时钟；`scenario_tests.rs` 声明用户旅程子模块。测试基础设施与场景 **MUST** 分离，二者均受 `cfg(test)` 约束，且 **NEVER** 使用 `mod.rs`。

### 4.6 渐进迁移

- 新增测试立即遵守本文规则；
- 修改旧模块时只迁移与本次行为相关的测试；
- 不产生行为变化的批量移动应独立 PR；
- 迁移不能削弱断言或删除原有边界场景；
- 现有测试文件路径不是永久兼容契约。

## 5. 命名与可追溯性

测试名采用“行为 + 条件 + 结果”：

```text
submit_when_idle_emits_user_message
escape_with_completion_open_closes_completion
escape_when_busy_without_completion_cancels_turn
missing_optional_field_deserializes_to_none
```

不强制 `test_` 前缀。Bug/feature 编号写入注释、issue 或 PR 关联，测试名表达长期稳定行为，避免 `test_bug_123_case_2` 一类失去语义的命名。

失败消息应描述期望不变量和关键上下文，避免只输出 `left != right`。异步 Harness 超限必须打印待处理事件、Effect、虚拟时间和核心状态。

## 6. Fixture、替身与确定性

### 6.1 Owning-layer fixture

测试基础设施跟随被测能力的架构层和模块：

```text
agent/features/runtime/src/domain/<module>/testing.rs + testing/
agent/features/runtime/src/application/<module>/testing.rs + testing/
agent/features/runtime/src/ports/<seam>/testing.rs + testing/
agent/features/runtime/src/adapters/<adapter>/testing.rs + testing/
agent/features/runtime/src/shared/<utility>/testing.rs + testing/  # 仅当 utility 真实属于 shared
agent/features/provider/src/<owning-layer>/<module>/testing.rs + testing/
apps/cli/src/tui/testing.rs + testing/
```

Runtime 当前 Target 在 crate 根采用 `domain/application/ports/adapters` 轻量六边形；fixture、Fake 和 Scripted Driver 必须留在其真实 owning layer。`shared` 不是默认必备层，也不是禁止项：只有内容被多个稳定 owner 使用、没有更明确的业务 owner，且抽取后依赖方向仍正确时才按需创建；**NEVER** 为目录对称预建空 `shared/`，也不得将暂时无法归类的代码、领域类型、Port、Adapter 或测试 fixture 塞入其中。

只有被同一 crate 多个层共同消费、且确实无领域和边界语义的测试基础能力，才可评审后放 crate-level `src/testing/`。测试 fixture 不因复用自动归入 `shared`。**NEVER** 建立知道所有领域类型、跨越架构层或构成第二套 VSA 的万能 `test_utils` / `testing`。

### 6.2 替身术语

| 名称 | 语义 |
|---|---|
| `StubX` | 返回固定结果，不模拟完整行为 |
| `FakeX` | 提供简化但可工作的实现 |
| `SpyX` | 记录调用供测试断言 |
| `ScriptedX` | 按预设事件/结果队列执行 |
| `MockX` | 仅用于 mock framework 生成对象 |

替身名字必须表达职责，禁止将所有测试客户端统称 `MockClient`。

### 6.3 确定性

- 时间、timer、TTL、超时比较 **MUST** 使用可注入 Clock/VirtualClock；
- 测试不得用短 `sleep` 或毫秒级墙钟差比较证明状态重置；
- ID 和随机源必须固定或注入；
- 不修改进程全局 cwd；
- 共享环境变量测试必须串行隔离，优先改为配置注入；
- 临时文件使用每测试唯一目录，禁止固定 `/tmp/aemeath_xxx`；
- 异步事件通过有上限的 `run_until`/脚本队列推进。

CI 首次失败后定向重跑只用于判断 flaky，**NEVER** 用重跑成功覆盖首次失败。flaky 测试必须修复确定性或登记阻断，不允许长期静默重试。

## 7. 覆盖率治理

### 7.1 工具与口径

[#677](https://github.com/rushsinging/aemeath/issues/677) 使用 `cargo-llvm-cov` 作为唯一 Rust 覆盖率工具，聚合 L1～L4：

- unit tests；
- crate integration tests；
- TUI/业务场景测试；
- 必要的 doc tests。

报告至少输出 line、region、function 三项；门禁优先采用 line + region，function 作为观察指标。

### 7.2 纳入与排除

纳入 workspace 自有 Rust crate。允许排除：

- generated code；
- `build.rs`；
- `main.rs` 中无逻辑的纯装配入口；
- 当前 runner 无法执行的平台专属分支；
- 有明确理由的 defensive unreachable 分支。

**NEVER** 默认排除全部 UI render、adapter 或 orchestration。它们应分别由 Buffer、契约和场景测试覆盖。排除清单集中维护并接受 code review，禁止散落 `#[coverage(off)]` 掩盖缺口。

### 7.3 门禁演进

#### 阶段 1：建立基线（#1014 已落地）

统一入口为 `./scripts/coverage.sh`，固定使用 `cargo-llvm-cov 0.8.7`，以 `target/coverage` 隔离普通构建。脚本运行 workspace 全部测试 target（包括 binary-only `cli`），仅在命令行打印 workspace 总体与 per-crate 的 line/region/function 摘要，不生成或上传覆盖率 artifact。

在 `release/v0.1.0` commit `259969fd` 上采集的初始基线：

| package | regions | functions | lines |
|---|---:|---:|---:|
| workspace | 68.95% | 69.45% | 68.29% |
| audit | n/a（无可计数项） | n/a（无可计数项） | n/a（无可计数项） |
| cli | 75.96% | 80.15% | 74.74% |
| composition | 26.37% | 29.41% | 23.19% |
| context | 69.12% | 65.79% | 67.72% |
| hook | 56.83% | 55.21% | 57.97% |
| logging | 69.75% | 76.04% | 71.39% |
| policy | 90.60% | 83.87% | 88.17% |
| project | 52.54% | 52.63% | 54.24% |
| provider | 45.13% | 46.51% | 47.59% |
| runtime | 65.33% | 64.95% | 65.26% |
| sdk | 81.36% | 75.98% | 81.42% |
| share | 83.17% | 85.24% | 81.84% |
| storage | 75.28% | 69.26% | 76.39% |
| tools | 49.70% | 49.07% | 49.44% |
| update | 54.01% | 30.77% | 46.70% |
| utils | 100.00% | 100.00% | 100.00% |

该基线只提供可见性，**NEVER** 在 #1014 设置统一百分比门禁。changed-lines、workspace 不下降和 per-crate 阈值由 #1018 基于后续稳定数据决定。

#### 阶段 2：阻止新增债务

- workspace line/region 不得显著下降；
- changed lines 设置较高覆盖要求；
- 新增核心逻辑必须有测试；
- 排除项变化必须显式 review。

#### 阶段 3：per-crate 阈值

根据真实基线分类设置阈值：

- Domain/纯逻辑采用高阈值；
- Application/Adapter/Runtime orchestration 采用中阈值；
- CLI/平台入口允许较低数值，但必须提供场景或 smoke 证据。

**NEVER** 用单一 workspace 百分比掩盖关键 crate 的低覆盖率。

### 7.4 覆盖率不等于生产可达性

覆盖率只回答“测试执行了哪些代码”。一个方法如果只被测试调用，可能有 100% 覆盖率，却仍是生产 dead code。因此覆盖率信号与生产可达性信号必须独立执行、独立失败。

## 8. 生产可达性与 dead code

### 8.1 禁止的第四类代码

代码只允许属于三类：

| 类型 | 生产构建 | 测试构建 | 治理方式 |
|---|---:|---:|---|
| 生产行为 | 是 | 是 | 必须有真实生产调用路径 |
| 测试基础设施 | 否 | 是 | `cfg(test)` 或明确 test-only feature |
| 对外公共契约 | 是 | 是 | API 清单、契约测试和明确所有者 |

**NEVER** 保留“进入生产 artifact、但只有测试调用的便利方法”。

### 8.2 测试辅助 API

只服务测试的 constructor、setter、状态读取器、fixture adapter 必须位于：

```text
#[cfg(test)] mod testing;
```

或 `#[cfg(test)] impl Type`。测试不得要求生产类型长期暴露 `for_test`、`set_x_for_test`、`text_snapshot` 一类入口。

若某生产方法只有测试引用：

1. 若行为不再需要，删除方法及其专属测试；
2. 若逻辑仍被上层行为需要，让测试通过真实生产入口覆盖；
3. 若内部计算有独立价值，将其下沉为被生产路径实际调用的纯函数；
4. 若属于外部契约，登记 API 所有者并用契约测试证明。

### 8.3 最小可见性

可见性按以下顺序选择：

```text
private → pub(super) → pub(crate) → pub
```

只有 Published Language、Port、SDK 或明确下游消费的稳定契约使用 `pub`。降低可见性可让 compiler/clippy 更准确识别无生产调用点的代码。

### 8.4 Production-only lint

本地/离线统一入口为 `cargo run -p xtask -- production-reachability .`，依次执行：

```text
cargo check --workspace
  → cargo clippy --workspace --bins --lib -- -D warnings
```

`--all-targets` 会让测试引用参与分析，不能替代 production-only gate。#1015 不新增 PR workflow；冷/热耗时和失败价值交由 #1018 决定在线 PR CI、离线/定时、Stop hook 或手动执行。历史 dead-code 清理由 #649/#947 承接。

### 8.5 Public API 与动态入口

真正的 `pub` API 可能供 workspace 外部使用，compiler 无法证明它无人调用。#1015 的 `cargo run -p xtask -- source-guard . <output>` 可生成 deterministic workspace public surface 文本供 PR diff；这只是可见性报告，不声明 crates.io semver 兼容门禁：

- 新增 API 必须说明生产/外部所有者；
- 删除/改变 API 必须评估兼容性；
- 非契约 API 优先降低可见性。

以下入口不能机械按文本引用删除：

- trait 实现；
- inventory/plugin 注册；
- serde 回调；
- feature-gated adapter；
- 平台实现；
- macro 展开入口；
- SDK 外部 API。

它们分别通过契约测试、注册表测试、feature matrix 和 public API 清单证明存活，禁止给整个模块添加 `allow(dead_code)`。

### 8.6 测试专属入口守卫

#1015 将 `.agents/hooks/check-production-reachability.sh` 注册进 Stop 架构守卫。该守卫调用 Rust `xtask source-guard`，机械检查：

- `for_test`、`set_*_for_test`、`test_only` 等入口不得出现在生产区域；
- `testing`、`fixture`、`fake` 模块必须受 `cfg(test)` 或批准的 test-only feature 约束；
- 生产模块不得依赖测试模块；
- 新增 `allow(dead_code)` 必须进入集中例外表；
- 测试 adapter 不得重新成为生产写入口。

## 9. CI 分层门禁

### 9.1 最终执行矩阵（#1018）

| 检查 | 执行位置 | 决策依据 |
|---|---|---|
| staged Rust fmt | Git pre-commit | 无网络、按路径触发、自动重新暂存 |
| source guard / dead-code baseline | Git pre-commit + pre-push full profile | 热约 3.7-6s，提交时按路径反馈，push 前完整兜底 |
| fast architecture guards | Agent Stop | 无 Cargo 的静态守卫，即时阻止架构债务 |
| snapshot 草稿 | Git pre-commit | 只查 `.snap.new` / `.pending-snap`，不冷编译 |
| workspace coverage | 现有 Coverage PR workflow | 约 2m54s，提供唯一 workspace/per-crate 全景 |
| production reachability | Git pre-push + Release Gate | 热 4.97s、冷 54.58s，不新增在线 workflow |
| TUI P0/snapshot | 本地合入前 | 热 0.97s、冷 82.75s，Coverage 会自然执行场景 |
| workspace tests | Git pre-push/本地合入前 | 冷 101.22s、热 5.28s |
| all-target clippy | 本地合入前 | 热 55.85s，在线收益不足以抵消重复编译 |
| changed-lines | 本地报告 | 首期只提供信号，不设阈值、不实时更新基线 |
| #677 文档—代码双向校验 | sub-issue 调整、PR 前后、父 Issue 关闭前人工执行 | 仅服务 #677 有限生命周期，不沉淀为长期 xtask 或 pre-commit |
| P1/PTY/platform | #1050 | 独立慢速能力，不伪装在 #1018 已覆盖 |

除 Coverage 外，本阶段不新增在线 workflow。`--no-verify` 只允许作为 Git 原生紧急绕过；PR Test plan 必须披露并补跑。

### 9.2 覆盖率 Job

`.github/workflows/coverage.yml` 在 Rust/Cargo、覆盖率脚本或自身发生变化的 PR 上执行：

- 安装固定版本 `cargo-llvm-cov 0.8.7` 与 `llvm-tools-preview`；
- 调用唯一入口 `./scripts/coverage.sh`；
- 在命令行打印 workspace 总体与 per-crate 的 line/region/function 摘要；
- 不生成或上传 HTML、LCOV 等覆盖率 artifact；
- 使用 `target/coverage` 隔离普通测试构建；
- 通过 workspace 测试自动包含 binary-only `cli` 的 binary unit tests 和后续 scenario targets。

### 9.3 慢速门禁

P1、feature/platform matrix 与真实 PTY smoke 由 #1050 落地为 `scripts/check-slow-test-matrix.sh`：

- host-native：fmt、workspace all-target clippy、workspace tests、TUI P0/P1、CLI build、真实 PTY smoke；
- cross target：设置 `AEMEATH_MATRIX_CROSS=1` 后按 macOS/Linux host 尝试双架构 build；target/linker 不可用时明确 `SKIP`，编译失败仍阻断；
- PTY 使用 allowlist 环境和隔离 HOME/agents config，验证 alternate screen 进入、Ctrl+C 退出、alternate screen/cursor 恢复，不访问真实 provider；子进程等待有上限，失败路径 kill/reap；
- host-native 各层只执行一次：workspace 排除 CLI，P0/P1 精确过滤，PTY 在 build 后单独执行；当前完整热运行 77.48s（其中 all-target clippy 为主要成本），PTY 约 2s、P1 约 0.04s；跨 target 首次运行因额外构建成本较高，仅手动/release 前执行；
- 不新增 PR workflow。

## 10. v0.1.0 落地关系

```text
#677 测试架构与覆盖率治理
  ├─ 测试分层和目录规范
  ├─ cargo-llvm-cov + PR workflow
  ├─ 覆盖率基线与差分门禁
  ├─ production reachability + public API 审计
  ├─ fixture、clock 与 flaky 治理
  └─ #1006 TUI TestBackend 场景测试
       ├─ 单帧驱动器
       ├─ Harness / Effect Driver
       ├─ P0/P1 场景
       └─ insta CI
```

TUI completion 的 idle/busy 回归应作为首个真实场景验收：Esc 在补全打开时只关闭补全；补全关闭且 Runtime busy 时才取消 turn。它同时要求 reducer Effect 断言和 TestBackend 完整链路检查，证明 L1 与 L4 没有互相替代。

推荐实施顺序：

1. 更新测试规范与目录规则；
2. 建立 production-only lint 和覆盖率基线；
3. 落地 TUI 单帧驱动器与 Harness；
4. 用真实回归补齐首批 P0 场景；
5. 启用 changed-lines 门禁；
6. 渐进清理 `include!`、固定 `/tmp`、真实墙钟和 test-only 生产 API。

## 11. 实现状态与验证证据

### 11.1 #1013 已对齐范围

`specs/rust-coding.md` 已承接本文的测试组织约束：

- L0-L5 测试层级及“最低充分层级 + 跨层相邻证据”；
- 按纯函数、状态机、Application Service、Adapter、序列化、TUI widget/场景选择覆盖证据；
- inline、同级 `*_tests.rs`、owning-layer 的 `tests.rs` + `tests/`、crate integration test，以及 `scenario_tests.rs` + `scenario_tests/` 目录规则；
- 所有测试模块遵循同名文件与目录并存，禁止 `mod.rs`；
- 测试和 fixture 跟随真实架构层/模块，禁止跨六边形层建立万能测试层；
- 行为 + 条件 + 结果命名、显式语义断言和 insta 的 TDD 流程；
- crate-local fixture、替身术语、VirtualClock、唯一临时目录、cwd/env 隔离和 flaky 首次失败保留；
- 渐进迁移，禁止为统一目录一次性搬迁全仓测试。

#1013 不改变 Rust 生产代码、历史测试结构或现有 Hook 实现。覆盖率、生产可达性、TUI Harness/P0 与 CI 收尾分别由 #1014～#1018 承接。

### 11.2 #1013 验证证据

#1013 在独立 worktree 完成以下验证：

- Markdown 相对链接检查通过；
- `specs/` 中“每公共函数固定三个测试、必须同文件末尾、UI render 整体豁免、强制 `test_` 前缀”等冲突规则检索为零；
- `git diff --check` 通过；
- `.agents/hooks/check-architecture-guards.sh` 全部通过；
- `.agents/hooks/check-unit-tests.sh` 首次执行通过：share 288、runtime 413、project 15、policy 18、context 111、provider 153、tools 168、storage 44、hook 50、audit 0、cli 952（其中 6 ignored），均为 0 failed。

### 11.3 #1015 已对齐范围与耗时

- `tools/xtask` 统一承载 coverage summary、production reachability、test-only API/dead-code baseline 和 public surface；#1014 的 Python 汇总器及测试已删除。
- `output_selection_view_for_test` 已限制为 `cfg(test)`；`set_permission_mode_for_test` 原本已受 `cfg(test)` 保护，无需行为修改。
- `.agents/dead-code-baseline.json` 记录当前 10 个生产 `allow(dead_code)` 上限、owner 与退出条件；新增数量由 pre-commit 的路径触发检查和 pre-push full profile 阻断，历史清理由 #649/#947 承接。
- public surface 仅生成 deterministic 文本（当前 1983 项）供 diff，不承诺 semver 门禁。
- production reachability 冷启动实测 54.58s（含全新 target 编译，首次因新暴露 unused import 失败并完成根因修正）；热启动 4.97s，其中 check 1.34s、clippy 2.41s。
- source guard 热启动 3.70s，其中扫描 3.10s。
- #1015 不新增 PR workflow；#1018 基于上述耗时决定最终执行位置。

### 11.4 #1017 已对齐范围与耗时

- CLI `dev-dependencies` 引入 insta；`scripts/check-tui-snapshots.sh` 以 `CI=1`、`INSTA_UPDATE=no` 运行 13 个 P0/基础场景并拒绝草稿文件。
- Harness 支持严格 Scripted Effect、Ui/Runtime 事件、step/drain/run_until(max_steps)、离散 tick 和诊断输出；不访问真实 TTY、网络、剪贴板或用户目录。
- 稳定快照覆盖 streaming thinking/completed、tool running/completed、AskUser shown/confirmed；每个场景先有 Effect/Model/屏幕语义断言。
- #1009 场景复现并根因修复 busy Enter 绕过 visible completion 的行为；Esc/Tab/busy cancel 均有完整链路断言。
- snapshot-check 冷启动 82.75s；热启动 0.97s；13 个测试本身约 0.06s。#1018 决定是否进入在线 PR CI。

- `.agents/flaky-debt.json` 集中记录真实墙钟、固定 `/tmp`、全局 env/cwd 与随机源风险，每项包含 owner Issue、风险和退出条件。
- `cargo run -p xtask -- run-test <retries> <command...>` 保留首次退出码；重跑成功只分类为 `flaky-suspect`，最终仍失败，**NEVER** 用重跑绿覆盖首次红。
- changed-lines 使用 `cargo run -p xtask -- changed-lines <coverage.json> <diff.patch>` 本地报告修改行覆盖和缺口；不设阈值、不上传 artifact、不实时回写 #1014 基线。
- `.cargo/hooks/pre-commit` 仅运行 staged fmt、条件 source guard 和 snapshot 草稿检查；GitHub Issue 治理不进入通用 hook。
- #677 双向校验仅在生命周期关键节点使用 `gh issue view` 和原生关系人工核验，不沉淀为长期 xtask。

### 11.5 #1018 收尾决策

- workspace tests 冷 101.22s / 热 5.28s；为避免每次 Agent Stop 重复冷编译，#1256 将其移至 Git pre-push；all-target clippy 热 55.85s，仍保留本地合入前执行，不新增在线 workflow。
- Agent Stop 只执行无 Cargo 的 fast architecture profile；pre-push 顺序执行 full architecture profile 与 workspace tests。pre-commit 和 xtask 单测仍为本地轻量门禁；Coverage 保持唯一新增在线 PR workflow。
- P1/PTY/platform 已创建原生 sub-issue #1050，blocked by #1018，并纳入 Release Gate 后续验收。
- #1014 文档基线是一次性参考快照；仅统计口径、工具版本、发版采样或用户明确要求时更新。

### 11.6 #983 AtomicDataset L0–L5 覆盖证据

#983 按最低充分层级覆盖独立 `AtomicDatasetPort` / 文件系统 adapter，不以单一 happy-path 测试替代相邻层证据：

- **L0**：production reachability、all-target clippy、public surface/source audit 与 architecture guards 验证窄 façade、`domain ← ports ← adapters` 方向；Guard exception / allowlist 净增 0。
- **L1**：覆盖 `DatasetKey` / member 校验、规范排序、空 dataset revision、顺序无关与事实敏感摘要、omitted 集合及纯恢复决策。
- **L2**：以临时目录覆盖 adapter 内 dataset lock、stage/fsync、manifest/journal、previous、promote/quarantine 与读取前恢复协作。
- **L3**：只经 crate-root 公共 API 运行 `AtomicDatasetPort` contract，覆盖首次空 dataset、完整 replacement、omitted delete、expected-revision CAS、Previous、promote 与 quarantine。
- **L4**：故障矩阵逐点覆盖 Prepared 前失败保留旧 generation、Prepared 后普通故障返回 committed `RecoveryPending`、reopen roll-forward，以及 journal/member 证据矛盾返回 typed `CorruptTransaction` 并 quarantine；每次读取只能得到完整旧代或完整新代。
- **L5**：真实子进程覆盖 OS lock，以及 durable Prepared / 中间 member publish 后 abort → reopen → roll-forward。

Memory active+archive 与 legacy key migration 的跨 BC 场景不属于 #983；该 integration deferred 至 [#896](https://github.com/rushsinging/aemeath/issues/896)，届时补 Memory 相邻层与场景证据。

### 11.7 #884 Tool Result materialization 覆盖证据

- **L1**：Config default/partial patch/invalid policy normalization，以及 Runtime materializer 的阈值边界、Unicode char preview、opaque locator 和写失败保留完整 inline。
- **L3**：`tool_result_blob_contract.rs` 以 fake `AtomicBlobPort` 验证 `StorageNamespace::ToolResult` key 映射、ProcessCrashSafe、write-once 幂等、同 ID 内容冲突与非法 segment fail-closed。
- **L4**：Main/Sub 两条生产路径均注入同一 materializer，现有 provider-id 与 oversized-result 场景证明输出一致；Storage 旧 helper/常量零引用由 crate API guard 证明。
- **兼容边界**：旧 session 中已持久化的 `.txt` 绝对引用仍是普通文本且不被迁移或删除；新 AtomicBlob locator 不承诺复用旧物理布局。

### 11.8 #1062 Policy L0–L5 覆盖证据

Policy v0.1.0 生产 `Standard` 与 `AllowAll` 两种授权上下文，`Deny` / `RequireApproval` 仍是 Published Language 的 Future 兼容变体。#1221 将原先单纯的 AllowAll decision 扩展为 Config 驱动的统一授权链，因此测试审查以“模式真相唯一、每个 ToolCall 只评估一次、授权上下文无损到达所有消费者、Main/Sub/MCP 行为一致”为边界：

| 行为 / 风险 | 必要层级 | 证据路径 | 结论 |
|---|---|---|---|
| Config Ask/AutoRead/AllowAll 映射为唯一 `PolicyMode`，Policy façade 保持窄且生产可达 | L0 / L3 | `agent/features/policy/tests/policy_contract.rs`；`check-unified-authorization.sh`、`check-crate-api-boundary.sh`、`check-production-reachability.sh` | 已覆盖 |
| `PolicyRequest` 保留 Run、Step、Tool、capabilities、workspace，空 workspace 拒绝 | L1 / L3 | `agent/features/policy/tests/policy_contract.rs`；`agent/features/runtime/src/application/tool_coordination_tests.rs` | 已覆盖 |
| Standard 保留五项授权 guard，AllowAll 关闭五项授权限制并对合法请求返回对应 `AuthorizationContext` | L1 / L3 | `agent/features/policy/tests/policy_contract.rs` | 已覆盖 |
| `ConfiguredPolicy` 每次评估读取 committed mode，动态更新对下一 ToolCall 生效 | L3 | `agent/features/policy/tests/policy_contract.rs` | 已覆盖 |
| Policy 日志只记录 mode、capability count、decision，不泄漏 Tool、路径或 ID，且全局 logger 测试串行确定 | L1 | `agent/features/policy/src/adapters/adapters_tests.rs` | 已覆盖 |
| Runtime 对每个 ToolCall 先评估 Policy，再按授权上下文执行或绕过 fuse；`Deny` / `RequireApproval` 映射为拒绝；目录缺失与非法请求不调用 Policy | L2 | `agent/features/runtime/src/application/tool_coordination_tests.rs` | 已覆盖 |
| Composition 注入的同一 `Arc<dyn PolicyPort>` 同时到达 Main 与 Sub runner | L2 | `agent/features/runtime/src/application/client/from_args.rs`；`agent/features/runtime/src/application/startup/runtime_support.rs` | 已覆盖 |
| `--yolo` 与 `--allow-all` 经过 CLI → SDK bootstrap 投影为同一兼容 ACL | L3 | `apps/cli/src/args.rs` | 已覆盖 |
| CLI/config AllowAll 对项目外路径、read-before-write、Bash safety、fuse 与 permission hooks 的完整授权旅程 | L4 | #1221 的 Tool/Project/Runtime 场景与统一授权 Guard；`agent/features/tools/src/adapters/*_tests.rs`、`agent/features/runtime/src/application/tool_coordination_tests.rs` | 已覆盖 |
| 真进程、PTY、网络、平台或发布资产 | L5 | Policy 与授权消费者均可由进程内契约/场景覆盖，无额外系统边界 | 不适用 |

覆盖率以 `./scripts/coverage.sh` 的实际输出为准；#1062 同步 #1221 后实测 policy 为 regions 86.30%、functions 76.92%、lines 85.07%。绝对百分比低于旧 AllowAll-only 基线 90.60% / 83.87% / 88.17%，原因是 #1221 新增 Standard/ConfiguredPolicy 与五维授权上下文后分母扩大；新增核心分支均有 L1～L4 证据，未覆盖项主要为简单 getter 与 defensive 分支。changed-lines 在 v0.1.0 仅记录信号、不设阈值；production reachability 与覆盖率独立执行。审查未发现需要新增 L5 或由新 Issue 承接的 Policy 关键行为缺口。

### 11.9 #1057 Storage 根因级测试审查执行计划

本节先冻结执行计划，不提前宣称 Storage 已通过父项验收。实施必须从行为—风险矩阵和失败证据出发；不得只补覆盖率数字，也不得用 L4/L5 替代 L1～L3。父 Issue [#848](https://github.com/rushsinging/aemeath/issues/848) 创建时的业务叶子 #991、#880、#881、#882、#883、#884、#983 均已关闭，#1057 负责核验这些交付组合后的测试完整性、确定性、生产可达性和治理退出证据。

#### 11.9.1 范围与审查单元

审查按稳定行为而非生产文件计数，覆盖以下八个单元：

1. `SafePathSegment`、`StorageKey`、`DatasetKey` 与 capability-root 路径安全；
2. `AtomicBlobPort` 的整值替换、代际读取、promote、quarantine、delete-all 与 list；
3. blob journal、durability、跨进程锁、提交点与 crash recovery；
4. `AtomicDatasetPort` 的 manifest、revision、完整 replacement、omitted delete 与 CAS；
5. dataset Prepared commit、roll-forward、promote、corruption quarantine 与跨进程隔离；
6. Context、Memory、Task、Audit、Runtime、Composition 对 Storage 机制的相邻边界；
7. Task/Memory/History 业务模型从 Storage 退役后的 production reachability 与公开面；
8. 测试 fixture、日志捕获、临时目录、子进程协调和平台差异的确定性。

每个单元建立「行为 / 风险 → 设计依据 → 必要层级 → 当前测试路径与测试名 → 已覆盖边界 → 缺口类型 → 修复路径或承接 Issue → 结论」矩阵。文档错误、实现缺口、测试缺口、过期测试和治理残留必须分开记录；发现业务实现或设计语义错误时，不得补一个适配错误现状的测试将其固化。

#### 11.9.2 L0～L5 责任

| 层级 | Storage 责任 | 计划证据 |
|---|---|---|
| L0 | production-only 编译、all-target lint、`domain ← ports ← adapters` 方向、窄公开面、test-only API、退役模型零回流 | production reachability、public surface/source guard、crate API guard、Storage layer guard、all-target clippy |
| L1 | 路径段/键校验、namespace policy、digest/revision、恢复决策、typed error 与局部不变量 | owning-layer `*_tests.rs` 或 `tests.rs + tests/`；先补失败证据再改实现 |
| L2 | adapter 内 lock、stage/fsync、journal/manifest、promote/quarantine、读取前恢复与 fault driver 协作 | 每测试唯一临时目录、受控 fault seam、有界子进程协调 |
| L3 | `AtomicBlobPort`、`AtomicDatasetPort`、`SafeStorageRoot` 及消费方窄 Port 的公共契约 | crate integration contract；共享断言定义一次，通过 factory/fixture 复用 |
| L4 | crash/abort → reopen → 完整旧代或新代、Storage 到消费方的关键恢复旅程 | 进程 harness 与相邻边界场景；逐边界保留 L2/L3 证据 |
| L5 | 仅真实平台 durability、安装或进程边界无法由 harness 证明时适用 | 默认不新增；现有真实子进程 crash/OS lock 先按 L4 process harness 归类，若平台能力仍不可证明再登记独立 smoke |

#### 11.9.3 根因级修正顺序

1. **冻结矩阵与失败基线**：逐项对照 #848 及七个已关闭业务叶子、Storage Target 文档、Published Language、Port、恢复状态表和 Current→Target 迁移记录，先记录不符合项。
2. **修复测试组织根因**：把 `lib.rs` 和 adapter 中违反当前规范的 inline test 迁回真实 owning layer；`domain_tests.rs` 按 `safe_path`、Published Language、blob recovery、dataset 等稳定 owner 拆分。只迁移本次涉及的测试，禁止顺带搬迁全仓历史测试；禁止 `mod.rs` 与 `include!`。
3. **修复测试基础设施根因**：日志捕获器安装失败必须显式失败，test fixture 保持 `cfg(test)`；Blob/Dataset 构造与恢复日志使用同一 owner 下的设施，避免全局状态静默失效。
4. **修复 flaky 根因**：锁测试用 child-ready/release 的有界握手直接证明 release 前未完成、release 后完成；墙钟只作死锁上限，不再以 `sleep + elapsed` 证明互斥。每个文件测试使用 RAII 唯一临时目录，不修改 cwd，不依赖用户目录。
5. **补齐 `SafePath` / `SafeStorageRoot`**：L1 覆盖合法等价类、非法输入、显示与键边界；L3 覆盖 root open、幂等多段 `ensure_dir`、中间目录 symlink、`open_existing`/`create_or_open` 的存在性与类型错误、entries 的 typed 分类/排序/过滤，以及错误与日志不泄漏绝对路径。Unix 特有证据显式标注平台边界。
6. **逐表复核 AtomicBlob**：确认 durability、提交前/后故障、Prepared/Committed digest 分支、跨 reopen promote、指定 generation quarantine、delete/list 与协议文件 no-follow；只对矩阵中的真实空白补测试。
7. **逐表复核 AtomicDataset**：确认 canonical revision、重复 member、stale CAS 零事务证据、omitted delete、显式 previous、完整 swap、fault matrix、corruption 优先级、quarantine 持续 fail-closed，以及相同/不同 dataset 的锁隔离。
8. **复核相邻消费边界**：Context Session snapshot、Memory dataset、Audit append path primitive、Runtime Tool Result 与 Composition wiring 必须各自保留相邻契约；Task/Memory/History 旧 Storage-owned schema 必须保持零生产可达。消费方业务缺陷另建 owner 明确的原生 sub-issue。
9. **收口公开面与 Guard**：核对 crate-root 与 `storage::api` 重复 façade、具体 filesystem adapter 的稳定性所有者、`check-crate-api-boundary.sh` 中过渡白名单及退役描述。若公开面迁移需要多消费方协同，拆为独立 sub-issue，不把结构迁移强塞进测试审查 PR。
10. **回写治理事实**：矩阵、覆盖率、production reachability、仍存缺口和 L5 不适用理由回写本文与 Migration Governance；同步 #848 的过期子项状态。存在未闭合关键行为时，#1057 与 #848 均不得宣称完成。

#### 11.9.4 验证与退出门禁

实施完成后按以下顺序保留首次结果：

1. `scripts/setup-dev-env.sh --check`；
2. `cargo fmt --all -- --check`；
3. `cargo check -p storage`；
4. `cargo test -p storage --lib` 与 `cargo test -p storage --tests`；
5. Context、Memory、Task、Audit、Runtime、Composition 的 Storage 相邻边界定向测试；
6. `cargo run -p xtask -- production-reachability .`；
7. `cargo clippy --workspace --all-targets -- -D warnings`；
8. `.agents/hooks/check-architecture-guards.sh`；
9. `cargo test --workspace`；
10. `./scripts/coverage.sh`，记录 Storage line/region/function 与 changed-lines 信号。

覆盖率与 production reachability 独立判定。首次失败不得被重跑成功覆盖；flaky 必须修复确定性或登记阻断。只有行为矩阵无未解释空白、关键缺口已补齐或有 owner 的原生 Issue 承接、全部适用门禁通过且父项治理事实已同步，才可给出 #848 的 Storage 测试完整性结论。

#### 11.9.5 #1057 实施结果与行为—证据矩阵

| 行为 / 风险 | 必要层 | 可追溯证据 | 结论 |
|---|---|---|---|
| SafePath、StorageKey、DatasetKey 的合法/非法等价类与策略不变量 | L1 | `agent/features/storage/src/domain/domain_tests.rs` | 已覆盖；补齐单字符、大小写/下划线、Unicode 与 `Display`，空键、namespace durability/previous policy、digest/revision/recovery 决策保持单元证据 |
| SafeStorageRoot capability-root、no-follow、普通文件与目录枚举 | L3 | `agent/features/storage/tests/safe_storage_root_contract.rs` | #1057 补齐；覆盖缺失 root、多段幂等目录、中间 symlink、create/open、missing/directory target、typed 排序/过滤及路径不泄漏 |
| AtomicBlob 整值替换、显式代际、promote/quarantine/delete | L2-L3 | `atomic_blob_contract.rs`、`crash_recovery.rs` | 完整；旧/新 primary、previous、指定代 quarantine、跨 reopen promote 幂等、并发 writer 与协议 symlink 均可追溯 |
| Blob 提交点、durability、fault matrix 与 typed corruption | L1-L4 | `domain/domain_tests.rs`、`crash_recovery.rs` | 完整；Prepared/Committed digest 分支、提交前 Err、提交后 warning、abort/reopen 与 corruption quarantine 均覆盖 |
| AtomicDataset manifest、revision、CAS、完整替换与 previous | L1-L3 | `domain/domain_tests.rs`、`atomic_dataset_contract.rs` | 完整；canonical revision、重复 member、omitted delete、stale CAS、显式 previous、promote/quarantine 均覆盖 |
| Dataset Prepared、roll-forward、完整代可见性与 corruption 优先级 | L2-L4 | `atomic_dataset_crash.rs` | 完整；member 中途发布、journal/member/revision 矛盾、partial quarantine fail-closed 与 promote crash matrix 均覆盖 |
| 相同 key/dataset 跨进程串行、不同 dataset 独立 | L4 | `crash_recovery.rs`、`atomic_dataset_crash.rs` | #1057 修复确定性；ready/release 有界握手替代 `sleep + elapsed` 业务断言，墙钟只保留等待上限 |
| Context Session、Memory dataset、Audit append primitive、Runtime Tool Result、Composition wiring | L3-L4 | `context/tests/session_snapshot_store_contract.rs`、`memory/src/adapters_tests.rs`、`audit/tests/append_store_contract.rs`、`runtime/tests/tool_result_blob_contract.rs`、`composition/tests/main_session_wiring.rs` | 已覆盖；#1057 增加 Session durability、promote、delete-all 相邻映射证据 |
| 测试 owning-layer、test-only API 与生产可达性 | L0 | Storage `*_tests.rs`、production reachability、source/architecture guards | #1057 收口；移除生产文件 inline test，domain/adapter/façade 测试外置；删除只为测试存在的 `SafePathSegment::name` API；日志捕获器安装失败不再静默 |
| crate-root / `storage::api` 重复 façade 与文档 `list_primary` 漂移 | L0-L3 | [#1263](https://github.com/rushsinging/aemeath/issues/1263) | 结构性缺口已由原生 sub-issue 承接并设置为 #1057 blocked-by；跨 Context/Config/Runtime/Memory/Composition，不在测试审查 PR 强行迁移 |
| 真终端、网络、安装与发布资产 | L5 | 不适用说明 | Storage 机制不依赖终端/网络/发布资产；真实子进程 crash 与 OS lock 已由隔离 process harness 覆盖，无新增 L5 |

测试组织与确定性结论：Storage domain 测试迁入 `src/domain/` owner，Blob/Dataset adapter 与 crate façade 日志测试外置；未新增 `mod.rs`、`include!`、万能 `test_utils` 或生产 test-only API。SafeStorageRoot 文件测试均使用 RAII 唯一目录。跨进程锁不再用固定等待时长判断成功。

验证证据（2026-07-20）：

- `cargo fmt --all -- --check`、`cargo check -p storage`、`cargo test -p storage --all-targets` 通过。
- Context Session、Runtime Tool Result、Memory、Audit append、Composition Main Session 定向测试通过。
- `cargo run -p xtask -- production-reachability .`、`cargo clippy --workspace --all-targets -- -D warnings`、全部 architecture guards 通过。
- 独立 `cargo test --workspace` 两次均在 Runtime 测试进程超过 10 分钟未结束；首次结果未被隐藏，也未以重跑成功冒充通过。`cargo test --workspace --no-run` 通过，已确认卡住进程不属于 Storage 变更路径。
- workspace `./scripts/coverage.sh` 同样因 Runtime 测试进程超过 10 分钟未完成；改用同版本工具执行 `cargo llvm-cov --manifest-path agent/features/storage/Cargo.toml --all-targets --summary-only`，Storage regions/functions/lines 为 `83.52% / 91.81% / 86.98%`。初始文档基线为 `75.28% / 69.26% / 76.39%`；口径分别记录，不把百分比当作行为正确证明。

最终结论：Storage 机制测试的 L0～L4 缺口已按最低充分层闭合，L5 不适用；公开面/list 契约漂移由 #1263 继续阻断 #1057，因此 #1057 与父项 #848 暂不满足关闭条件。

#### 11.10 #1058 Task Management L0–L5 覆盖证据

#849 创建时的直接执行叶子 #996、#885–#891 均已关闭；本审查只核验其组合后的 Task BC，不承载新的业务设计。审查基线为 Task-owned `domain + adapters`、`TaskAccess` / `TaskPersist` 两个窄 OHS，以及 Context Session 内嵌快照。

| 行为 / 风险 | 必要层 | 可追溯证据 | 结论 |
|---|---|---|---|
| typed ID、创建规格、状态机、DAG、Batch/current_batch、revision、删除边清理与 lifecycle | L1 | `agent/features/task/src/domain/{model,state,lifecycle,query}_tests.rs` | 已覆盖：合法/非法迁移、幂等、overflow、环、跨 Batch、blocked admission、稳定排序与 tombstone 过滤均有确定性断言。 |
| TaskSnapshot V1→V2 codec、typed validation、未知字段、wire ID、候选安装与稳定快照顺序 | L1/L2 | `agent/features/task/src/domain/{snapshot,snapshot_validation}_tests.rs`；`agent/features/task/src/adapters/snapshot_store_tests.rs` | 已覆盖：codec 与 aggregate validation 分阶段；#1058 补 live snapshot 的 Task/Batch typed-ID 稳定排序，避免 HashMap 枚举顺序泄漏。 |
| TaskAccess 单一事务 backing、同 backing Access/Persist view、失败/no-op 原子性 | L2/L3 | `agent/features/task/src/adapters/{contract/task_access.rs,store_tests.rs,wiring.rs}`；`agent/features/task/tests/task_persist_contract.rs` | 已覆盖：新增公共 `TaskPersist` integration contract，验证非空跨 view round-trip 与非法快照不修改 live view。 |
| Tool wire → Task ACL：非零 ID、已删除任务隐藏、创建/更新/停止/列表的意图命令 | L2/L3 | `agent/features/tools/src/adapters/task_{create,get,list,stop,update}_tests.rs`；`agent/features/tools/src/domain/types/task_{get,list}_tests.rs` | 已覆盖：#1058 将 Tool 输入统一委托 Task-owned non-zero parser，拒绝 `0`；TaskGet 不发布 tombstone；TaskView wire 仍锁定既有 batch 数字兼容格式且不含 owner。 |
| Runtime Task snapshot 渲染与 reminder 观察 | L1 | `agent/features/runtime/src/application/chat/looping/{task_snapshot_tests.rs,task_reminder_tests.rs}` | 已覆盖：状态排序、依赖渲染、截断/hidden count、零行限制、全部 Task mutation 工具与跨 turn 保留均有确定性断言；TaskStop 与 TasksSnapshot 的 mutation 集保持一致。 |
| Context Session 的 captured empty/missing/non-empty Task snapshot、Task prepare 失败的联合恢复原子性 | L3/L4 | `agent/features/context/tests/main_session_wiring.rs` | 已覆盖：新增 captured non-empty snapshot → TaskAccess 可观察恢复；self dependency prepare 失败时 canonical session、memory 和 Task state 均保持旧值。 |
| TaskCommandResult 事件到 Runtime/SDK/TUI 的唯一投影 | L4 | #879 | **实现缺口**：Task BC 已原子产出 `TaskEvent`，但当前 Runtime 仍按 Tool 名和结果文本推断 snapshot / hook，未消费事件建立权威 SDK 投影；由已开启的 #879 承接，#1058 / #849 因此保持开放。 |
| L0：生产可达性、公开面、Task persistence authority、layout/dependency 守卫 | L0 | `cargo run -p xtask -- production-reachability .`；`cargo clippy --workspace --all-targets --all-features -- -D warnings`；`.agents/hooks/check-architecture-guards.sh --full` | 已覆盖：Task target layout 与 Access/Persist capability policy 无新增 migration exception。 |
| 真进程、PTY、网络、平台、安装或发布资产 | L5 | 不适用说明 | Task BC 是进程内同步聚合、codec 和窄端口；真实外部边界不属于该能力，L1–L4 可完整覆盖，不新增 smoke。 |

确定性与组织：Task codec/aggregate 测试只用固定 timestamp/ID；Context 场景使用独立临时目录与 gate；本次新增 Tool/Runtime 测试均按同级 `*_tests.rs` 外置，未新增 `mod.rs`、`include!`、万能 fixture 或生产 test-only API。`TaskId::new(0)` 保留给 Snapshot validator 构造非法持久化 fixture；外部 Tool wire 只可经 `TaskId::parse_tool_input` 取得非零 ID。

覆盖率信号（2026-07-20，`./scripts/coverage.sh`）：Task regions **95.69%**、functions **93.85%**、lines **96.85%**。百分比只作风险信号；关键状态机、契约与跨 BC 恢复行为以本矩阵为验收依据。慢速矩阵的 PTY smoke 首次因未构建 CLI binary 失败，按 worktree-local Cargo build-dir 显式设置 `AEMEATH_PTY_BIN` 后通过；该 PTY 责任与 Task BC 无关，不影响 L5 不适用判断。

#### 11.11 #1060 Memory / Reflection L0–L5 覆盖证据

父 Issue [#851](https://github.com/rushsinging/aemeath/issues/851) 创建时的执行叶子 #895–#900、#984 与 #997 均已关闭。审查后新增的 #1283、#1284、#1285 已分别由 PR #1287、#1290、#1291 合入 `main`；Manual Reflection 的用户命令与 SDK/TUI 投影属于交付层工作，已移至 #860 的子项 #1289，不阻断本父项测试审查。

| 行为 / 风险 | 必要层 | 可追溯证据 | 结论 |
|---|---|---|---|
| MemoryId / Entry 不变量、eligibility、dedup、eviction、JSON parse、prompt/summary 与安全错误类别 | L1 | `agent/features/memory/src/domain/{model,persistence,reflection}.rs` 的现有单元测试；`agent/features/memory/tests/reflection_error_boundary.rs` | 已覆盖主要值对象、持久化规则与 parser 等价类；#1283 将 parse error 收窄为稳定 Display，并锁定模型正文不泄漏。`policy.rs` 的 eligibility/eviction/Jaccard 边界尚无直接单元测试，由 #1299 补齐。 |
| MemoryService candidate→CAS→publish、committed receipt、一次 recompute、query 零 I/O、reflection apply | L2 | `agent/features/memory/src/service.rs` 的 scripted-store 测试 | 已覆盖 commit failure、RecoveryPending、一次/二次 CAS、layer-targeted commit 与 query 不访问 Store。部分 apply 与 mixed suggestion/outdated 的事务边界仍由 #1299 补齐。 |
| MemoryPort / NoOp / opener / history adapter 的稳定边界、ACL 与持久化 | L3 | `agent/features/memory/tests/{memory_port_contract,noop_memory_contract,opener_seam_contract,reflection_history_adapter,reflection_error_boundary}.rs` | 已覆盖 Disabled NoOp、typed query/mutation、history reopen/upsert/corruption、history query 仅安全摘要。history CAS retry、opener 错误矩阵与完整 shared contract 仍由 #1299 补齐。 |
| Main Session 同一 active Arc、Context 只读注入、Sub Disabled / Shared、Reflection 端到端生命周期 | L4 | `agent/composition/src/memory.rs` 的 `main_views_share_the_active_arc`、`prepare_does_not_change_active_until_install`、`preparing_the_active_identity_reuses_arc_without_opening`、`sub_disabled_is_noop_and_shared_reuses_active_without_opening`；`agent/composition/tests/main_session_wiring.rs`；`agent/features/context/src/adapters/memory_injection.rs` 测试；`agent/features/runtime/src/application/chat/looping/pre_compact_trigger_tests.rs`；`agent/features/runtime/tests/reflection_teardown.rs` | #1284 证明 compact 成功后的冻结 snapshot 才提交 PreCompact；#1285 证明 grace deadline 后 cancel 并等待 terminal completion。submit_complete Running→terminal history、busy skip 不写 history 与 resume 后 Context adapter 边界仍由 #1299 补齐。 |
| L0 production reachability、all-target clippy、public/test-only API、architecture guards | L0 | `cargo run -p xtask -- production-reachability .`；`cargo clippy --workspace --all-targets -- -D warnings`；`.agents/hooks/check-architecture-guards.sh --full` | 2026-07-20 在 `89ac5d7e` 上通过；Rust `pub` API 不因零 workspace 调用自动判定 dead code，`for_complete_reflection` 当前无调用者、且 production adapter stub 不承载端到端提交，已作为死代码/接线风险由 #1299 承接。 |
| 真实 PTY / 平台 / 安装路径 | L5 | `apps/cli/tests/pty_smoke.rs`；`scripts/check-slow-test-matrix.sh` | 显式传入 worktree-local `AEMEATH_PTY_BIN` 后 PTY smoke 通过。慢速矩阵脚本硬编码 `$ROOT/target/debug/aemeath`，与 worktree Cargo target-dir 不兼容，首次失败保留并由 #1298 承接。 |

确定性与组织：#1283/#1284/#1285 的新增测试均使用外置 integration 或 owning module 测试文件；无 `mod.rs`、`include!` 或生产 test-only API。Reflection task 测试使用 cancellation token、Notify 和 bounded timeout，不以短 sleep 证明状态。

覆盖率信号（2026-07-20，`./scripts/coverage.sh`，commit `89ac5d7e`）：Memory regions/functions/lines **87.27% / 84.42% / 87.09%**；Runtime **71.66% / 71.10% / 72.11%**；workspace **77.77% / 78.99% / 78.07%**。原始命令行摘要已回写 #1060 comment；统一工具当前不生成 changed-lines 指标。Runtime 低于 workspace / Memory 的覆盖率仅作风险信号，不能替代上述行为矩阵，#1299 负责以行为缺口补证而非以百分比追数。

当前结论：#851 的初始业务叶子已关闭，已合入的 #1283/#1284/#1285 修复了审查发现的三项业务缺口；但 #1299 的分层测试缺口与 #1298 的慢速矩阵入口缺陷均有 owner 且仍开放。因此 #1060 / #851 暂不满足关闭条件，待两个承接项完成、复核 L0-L5 证据后再给出最终验收结论。

## 12. 相关文档

- [01-architecture-guards.md](01-architecture-guards.md)：架构守卫注册表与例外治理
- [../02-modules/tui/05-e2e-scenario-testing.md](../02-modules/tui/05-e2e-scenario-testing.md)：TUI 进程内 E2E 场景测试
- [../../superpowers/specs/2026-05-27-tui-model-view-architecture.md](../../superpowers/specs/2026-05-27-tui-model-view-architecture.md)：TUI Model/View 历史设计依据

## 13. 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-20 | #1060 初次 Memory / Reflection L0–L5 审查经独立复核后修正证据路径：`policy.rs` 无直接单元测试；same-Arc / Sub Disabled 正确证据在 `composition/src/memory.rs`；`for_complete_reflection` 零调用者与 Runtime 覆盖率风险均由 #1299 承接 | [#1060](https://github.com/rushsinging/aemeath/issues/1060)、[#1299](https://github.com/rushsinging/aemeath/issues/1299) |
| 2026-07-20 | #1060 完成 Memory / Reflection 初次 L0–L5 审查：记录 Memory/Runtime coverage、#1283/#1284/#1285 合入证据与 PTY worktree binary 首次失败；事务/history 相邻测试由 #1299、慢速矩阵路径修复由 #1298 承接，#851 暂不关闭 | [#1060](https://github.com/rushsinging/aemeath/issues/1060)、[#851](https://github.com/rushsinging/aemeath/issues/851)、[#1298](https://github.com/rushsinging/aemeath/issues/1298)、[#1299](https://github.com/rushsinging/aemeath/issues/1299) |
| 2026-07-14 | 初稿：定义六层测试模型、目录组织、覆盖率、生产可达性、dead-code 与 CI 治理 | [#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006) |
| 2026-07-15 | 将 L0-L5、覆盖证据、目录、命名、fixture 与确定性规则同步到 Rust 编码规范，并按 Runtime 单能力轻量六边形 Target 收敛测试归属；`shared` 仅在存在真实共享内容时按需创建 | [#1013](https://github.com/rushsinging/aemeath/issues/1013)、[#1027](https://github.com/rushsinging/aemeath/pull/1027) |
| 2026-07-15 | 接入 cargo-llvm-cov 0.8.7，建立 workspace/per-crate 命令行覆盖率入口与 v0.1.0 基线 | [#1014](https://github.com/rushsinging/aemeath/issues/1014) |
| 2026-07-15 | 用 Rust xtask 统一覆盖率汇总与生产可达性，落地 test-only API、dead-code baseline 和 public surface 本地/Stop 守卫 | [#1015](https://github.com/rushsinging/aemeath/issues/1015) |
| 2026-07-15 | 落地 TUI P0 Scripted Harness、稳定快照、本地草稿检查与 completion 回归 | [#1017](https://github.com/rushsinging/aemeath/issues/1017) |
| 2026-07-17 | 登记 #884 Tool Result 的 L1/L3/L4 覆盖：Config policy、Unicode materialization、写失败 fallback、AtomicBlob adapter contract、Main/Sub 共享入口与旧 `.txt` 引用兼容边界 | [#884](https://github.com/rushsinging/aemeath/issues/884) |
| 2026-07-17 | 登记 #983 AtomicDataset 的 L0–L5 覆盖：纯规则、adapter 协作、公共 port contract、Prepared/roll-forward/corruption fault matrix 与真实进程 abort/OS lock；Memory 集成 deferred 至 #896 | [#983](https://github.com/rushsinging/aemeath/issues/983) |
| 2026-07-19 | 完成 #1062 Policy 测试审查：登记 Standard/AllowAll Config 映射、五维授权上下文、Runtime 单次评估与 fuse、Main/Sub 同实例注入、CLI ACL、L4 授权旅程及 L5 不适用理由 | [#1062](https://github.com/rushsinging/aemeath/issues/1062)、[#1221](https://github.com/rushsinging/aemeath/issues/1221) |
| 2026-07-20 | 完成 #1057 Storage 根因级测试审查：补齐 SafeStorageRoot、Session 相邻映射、owning-layer 与锁确定性；记录 Storage 覆盖率和 workspace Runtime 卡住事实；公开面/list 契约漂移由 #1263 承接 | [#1057](https://github.com/rushsinging/aemeath/issues/1057)、[#1263](https://github.com/rushsinging/aemeath/issues/1263) |
| 2026-07-20 | 完成 #1058 Task Management 测试审查：补 Task Tool ACL、TaskPersist contract、Context restore、Runtime snapshot/reminder 与稳定 snapshot；TaskEvent→Runtime/SDK/TUI 唯一投影缺口由 #879 原生依赖承接 | [#1058](https://github.com/rushsinging/aemeath/issues/1058)、[#849](https://github.com/rushsinging/aemeath/issues/849)、[#879](https://github.com/rushsinging/aemeath/issues/879) |
| 2026-07-20 | 冻结 #1057 Storage 根因级测试审查计划：按八个稳定行为单元建立 L0～L5 矩阵，优先修复 owning-layer、日志测试设施、墙钟锁断言与 SafeStorageRoot 契约根因，再复核 Blob/Dataset、消费方边界、公开面和 Guard | [#1057](https://github.com/rushsinging/aemeath/issues/1057)、[#848](https://github.com/rushsinging/aemeath/issues/848) |
