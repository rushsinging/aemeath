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
| source guard / dead-code baseline | Git pre-commit + Stop hook | 热约 3.7-6s，机械阻止新债 |
| snapshot 草稿 | Git pre-commit | 只查 `.snap.new` / `.pending-snap`，不冷编译 |
| workspace coverage | 现有 Coverage PR workflow | 约 2m54s，提供唯一 workspace/per-crate 全景 |
| production reachability | 本地合入前 + Release Gate | 热 4.97s、冷 54.58s，不新增在线 workflow |
| TUI P0/snapshot | 本地合入前 | 热 0.97s、冷 82.75s，Coverage 会自然执行场景 |
| workspace tests | Stop/本地合入前 | 冷 101.22s、热 5.28s |
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
- `.agents/dead-code-baseline.json` 记录当前 10 个生产 `allow(dead_code)` 上限、owner 与退出条件；新增数量被 Stop 守卫阻断，历史清理由 #649/#947 承接。
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

- workspace tests 冷 101.22s / 热 5.28s；all-target clippy 热 55.85s，均保留本地/Stop/合入前，不新增在线 workflow。
- pre-commit 和 xtask 单测均为本地轻量门禁；Coverage 保持唯一新增在线 PR workflow。
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

## 12. 相关文档

- [01-architecture-guards.md](01-architecture-guards.md)：架构守卫注册表与例外治理
- [../02-modules/tui/05-e2e-scenario-testing.md](../02-modules/tui/05-e2e-scenario-testing.md)：TUI 进程内 E2E 场景测试
- [../../superpowers/specs/2026-05-27-tui-model-view-architecture.md](../../superpowers/specs/2026-05-27-tui-model-view-architecture.md)：TUI Model/View 历史设计依据

## 13. 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-14 | 初稿：定义六层测试模型、目录组织、覆盖率、生产可达性、dead-code 与 CI 治理 | [#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006) |
| 2026-07-15 | 将 L0-L5、覆盖证据、目录、命名、fixture 与确定性规则同步到 Rust 编码规范，并按 Runtime 单能力轻量六边形 Target 收敛测试归属；`shared` 仅在存在真实共享内容时按需创建 | [#1013](https://github.com/rushsinging/aemeath/issues/1013)、[#1027](https://github.com/rushsinging/aemeath/pull/1027) |
| 2026-07-15 | 接入 cargo-llvm-cov 0.8.7，建立 workspace/per-crate 命令行覆盖率入口与 v0.1.0 基线 | [#1014](https://github.com/rushsinging/aemeath/issues/1014) |
| 2026-07-15 | 用 Rust xtask 统一覆盖率汇总与生产可达性，落地 test-only API、dead-code baseline 和 public surface 本地/Stop 守卫 | [#1015](https://github.com/rushsinging/aemeath/issues/1015) |
| 2026-07-15 | 落地 TUI P0 Scripted Harness、稳定快照、本地草稿检查与 completion 回归 | [#1017](https://github.com/rushsinging/aemeath/issues/1017) |
| 2026-07-17 | 登记 #884 Tool Result 的 L1/L3/L4 覆盖：Config policy、Unicode materialization、写失败 fallback、AtomicBlob adapter contract、Main/Sub 共享入口与旧 `.txt` 引用兼容边界 | [#884](https://github.com/rushsinging/aemeath/issues/884) |
| 2026-07-17 | 登记 #983 AtomicDataset 的 L0–L5 覆盖：纯规则、adapter 协作、公共 port contract、Prepared/roll-forward/corruption fault matrix 与真实进程 abort/OS lock；Memory 集成 deferred 至 #896 | [#983](https://github.com/rushsinging/aemeath/issues/983) |
