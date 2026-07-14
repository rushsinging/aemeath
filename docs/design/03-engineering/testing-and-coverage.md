# 测试架构与覆盖率治理

> 层级：03-engineering（横切工程关注点）
> 状态：Target｜Milestone：v0.1.0｜对应 Issue：[#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006)
> 本文定义 workspace 统一测试分层、目录组织、fixture/替身、覆盖率、生产可达性与 CI 门禁。具体编码约束最终同步到 `specs/rust-coding.md`。

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
| L2 | 模块协作测试 | 同一 crate 内 service、port、reducer、assembler 的协作 | `src/<module>/tests/` |
| L3 | 契约测试 | Published Language、Port/Adapter、序列化和兼容性 | crate 根 `tests/`、contract suite |
| L4 | 场景测试 | 跨多个内部层的用户或业务旅程 | `scenario_tests/` |
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
src/business/chat/
  service.rs
  reducer.rs
  tests/
    mod.rs
    submit.rs
    cancel.rs
    compact.rs
```

一个文件对应一个稳定行为或用户故事，测试目录通过正常 Rust 模块树挂载。

**NEVER** 使用 `include!("tests/*.rs")` 拼接测试文件。`include!` 共享隐式作用域，降低 IDE、诊断、模块归属和覆盖报告可读性；现存用法按相关模块变更渐进迁移。

### 4.4 契约测试：crate 根 `tests/`

```text
packages/sdk/tests/published_language_compat.rs
agent/features/provider/tests/provider_contract.rs
```

crate 根 integration test 只能通过公共 API 验证契约。若契约需要多个实现共享，应暴露最小 test factory，而不是扩大生产 API。

### 4.5 场景测试：专用模块

```text
apps/cli/src/tui/testing/
apps/cli/src/tui/scenario_tests/
```

`testing/` 保存 Harness、Fake、fixture 和虚拟时钟；`scenario_tests/` 保存用户旅程。测试基础设施与场景 **MUST** 分离，二者均受 `cfg(test)` 约束。

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

### 6.1 Crate-local fixture

测试基础设施按领域归属：

```text
runtime/src/testing/
provider/src/testing/
apps/cli/src/tui/testing/
```

**NEVER** 建立知道所有领域类型的全仓万能 `test_utils`。共享基础设施仅承载真正跨领域且无业务语义的能力，例如确定性 ID 或临时目录封装。

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

#### 阶段 1：建立基线

- 生成 workspace 总体与 per-crate 报告；
- 上传 HTML/LCOV artifact；
- 记录 v0.1.0 基线；
- 不用主观全仓阈值阻断历史债务。

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

CI 必须先在**不编译测试 target**的生产视角执行 check/clippy，再执行 all-target lint：

```text
production-only check/lint
  → all-target clippy
  → tests
  → coverage
```

`--all-targets` 会让测试引用参与分析，不能替代 production-only gate。生产视角启用 `dead_code`、`unreachable_pub` 和适用的 unused lint；例外只允许落在最小符号上并说明 owner、原因和退役条件。

### 8.5 Public API 与动态入口

真正的 `pub` API 可能供 workspace 外部使用，compiler 无法证明它无人调用。CI 应生成 public API diff（例如 `cargo-public-api`）供 review：

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

架构守卫应检查：

- `for_test`、`set_*_for_test`、`test_only` 等入口不得出现在生产区域；
- `testing`、`fixture`、`fake` 模块必须受 `cfg(test)` 或批准的 test-only feature 约束；
- 生产模块不得依赖测试模块；
- 新增 `allow(dead_code)` 必须进入集中例外表；
- 测试 adapter 不得重新成为生产写入口。

## 9. CI 分层门禁

### 9.1 PR 快速门禁

1. `cargo fmt --check`；
2. production-only check/clippy；
3. `cargo clippy --workspace --all-targets -- -D warnings`；
4. architecture guards 与 test-only API guard；
5. `cargo test --workspace`；
6. P0 场景测试；
7. changed-lines coverage；
8. 拒绝 `.snap.new` / `.pending-snap`。

### 9.2 覆盖率 Job

独立安装固定版本 `cargo-llvm-cov`：

- 生成 workspace LCOV/HTML；
- 输出总体与 per-crate 摘要；
- 上传 artifact；
- 使用独立 cache/target 避免与普通测试污染；
- 对 binary-only `cli` 显式包含 binary unit/scenario targets。

### 9.3 慢速门禁

合入 release 分支或定时运行：

- P1 场景；
- feature/platform 矩阵；
- PTY smoke；
- 全量覆盖率；
- flaky 检测和测试耗时趋势。

Release workflow 保留完整验证，但不得成为首次发现普通 PR 回归的唯一门禁。

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

## 11. 相关文档

- [architecture-guards.md](architecture-guards.md)：架构守卫注册表与例外治理
- [../02-modules/tui/05-e2e-scenario-testing.md](../02-modules/tui/05-e2e-scenario-testing.md)：TUI 进程内 E2E 场景测试
- [../../superpowers/specs/2026-05-27-tui-model-view-architecture.md](../../superpowers/specs/2026-05-27-tui-model-view-architecture.md)：TUI Model/View 历史设计依据

## 12. 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-14 | 初稿：定义六层测试模型、目录组织、覆盖率、生产可达性、dead-code 与 CI 治理 | [#677](https://github.com/rushsinging/aemeath/issues/677)、[#1006](https://github.com/rushsinging/aemeath/issues/1006) |
