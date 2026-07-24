# OpenAI Reasoning Effort 映射修正实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 reasoning token budget 不再推导 effort，并让 OpenAI driver 正确接受 `none`、`minimal`、`low`、`medium`、`high`、`xhigh`、`max`，同时锁定其他 driver 的既有 capability 与 wire 行为。

**Architecture:** Shared Kernel 的有序 `ReasoningLevel` 增加 `Minimal`，并把禁用态的外部别名 `none` 解析为既有 `Off`；领域显示仍使用 `off`，OpenAI driver 在 wire ACL 中把 `Off` 映射为 `none`。`ThinkingBudget` 保留为独立协议配置，只表达预算或开关，不再进入 `ReasoningLevel` 解析、`as_effort` 或 driver clamp；OpenAI Chat 与 Responses 均从 invocation scope 的 effective level 生成 effort。其他 driver 继续从各自唯一 `ReasoningCapability` 派生 maximum/clamp，使用回归矩阵证明没有变化。

**Tech Stack:** Rust workspace、serde/serde_json、Provider hexagonal adapter、Cargo test/clippy、TDD。

**Issue:** [#1393](https://github.com/rushsinging/aemeath/issues/1393)（milestone `v0.1.0 — Context Engineering + 架构重构`）

---

## 设计决策与边界

1. **`none` 不新增第二个禁用领域值。** `none` 是 OpenAI wire 对禁用态的命名；共享领域继续以 `Off` 表示关闭。`ReasoningLevel::parse("none")` 接受配置别名并返回 `Off`，`Off::as_str()` 仍为 `"off"`，避免改变 Anthropic、Ollama、Workflow、Runtime 与持久化中的既有术语。
2. **`minimal` 是真实的有序 effort。** 在 `Off` 与 `Low` 之间新增 `Minimal`，否则配置中的 `minimal` 无法经过 Runtime/Provider capability clamp 到达 OpenAI wire。新增档位属于 Shared Kernel 跨 BC 变更，必须同步相邻 parser/capability 测试和 Target 文档。
3. **token budget 与 effort 正交。** 删除 `effort_from_thinking_tokens`。`ReasoningConfig::ThinkingBudget` 不再由 token 数量生成 `ReasoningLevel` 或 effort：legacy client 装配时仅按 `tokens == 0` 判定 `Off`，正数预算使用 boolean fallback level；请求编码时保留 driver 原有 budget/toggle 语义，但不得生成 `effort` / `reasoning_effort`。
4. **只让 OpenAI 声明完整新档位。** OpenAI capability 改为 `Off, Minimal, Low, Medium, High, Xhigh, Max`。其他 effort driver 的集合由现有 helper 继续生成，但需明确过滤 `Minimal`，避免共享枚举扩展后静默扩大 Zhipu、LiteLLM、Volcengine、DeepSeek 的可表达集合。
5. **Chat 与 Responses 共用 wire 映射。** 在 `ChatApiDriver` 增加从 effective `ReasoningLevel` 到 wire effort 的方法；默认使用领域字符串，OpenAI 覆写 `Off -> none`。Chat 配置投影和 Responses body 均调用该方法，不复制 OpenAI 特例。
6. **不做真实上游 smoke。** 这是确定性的请求映射变化，L1-L3 足以验证；不发送付费请求。

## 文件结构

- Modify: `agent/shared/src/reasoning.rs`
  - 增加 `Minimal`，接受 `none` 作为 `Off` 的解析别名，保持 `Off` 的 canonical string 为 `off`。
  - 将现有内联测试迁到 `reasoning_tests.rs`，符合当前 no-inline-tests 守卫。
- Create: `agent/shared/src/reasoning_tests.rs`
  - 覆盖 canonical round-trip、`none` alias 与新增排序关系。
- Modify: `agent/shared/src/config/domain/models/types.rs`
  - 更新 `reasoning_effort` 支持值说明，明确 `none` 是 `off` 的 OpenAI-compatible alias。
- Modify: `agent/features/provider/src/adapters/client.rs`
  - 解除 `ThinkingBudget(tokens)` 到 effort level 的分段映射；仅区分零预算关闭与正预算启用 fallback。
- Modify: `agent/features/provider/src/adapters/openai_compatible/driver.rs`
  - 删除 token-budget-to-effort 函数；添加显式 capability level helper和 wire effort 投影；OpenAI 声明完整档位与 `Off -> none`。
- Modify: `agent/features/provider/src/adapters/openai_compatible/reasoning.rs`
  - `ThinkingBudget` 不再从 `as_effort`、`for_scope`、测试 clamp 中产生 effort。
- Modify: `agent/features/provider/src/adapters/openai_compatible.rs`
  - 删除已退役 helper re-export。
- Modify: `agent/features/provider/src/adapters/openai_compatible/responses.rs`
  - Responses 请求通过 driver wire effort 投影，覆盖 `minimal/xhigh/max`，并保持 `Off` 不发送 reasoning 对象。
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/clamp_effort.rs`
  - OpenAI 全档位回归；其他 driver capability/clamp 快照。
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/reasoning.rs`
  - ThinkingBudget 不生成 effort；OpenAI Chat 全档位请求投影。
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/provider_config.rs`
  - 保留并强化非 OpenAI driver wire 回归，证明 ThinkingBudget 的 toggle 行为未被误删。
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests.rs`
  - 将现有 `include!` 测试迁为显式 `#[path] mod`，满足相关模块变更时渐进清理测试组织债。
- Create: `agent/features/provider/src/adapters/openai_compatible/tests/responses_reasoning.rs`
  - 独立覆盖 Responses effort 映射，避免继续扩大生产文件内联测试。
- Modify: `agent/features/provider/src/ports.rs`
  - 更新 façade 层 ReasoningLevel 字符串、解析、排序与 serde 测试。
- Modify: `agent/features/provider/src/published_language.rs`
  - 更新 resolver 测试矩阵，证明 `Minimal` 只在 capability 显式声明时可达。
- Modify: `docs/design/01-system/03-context-map.md`
  - Shared Kernel 枚举更新为 `Off / Minimal / Low / Medium / High / Xhigh / Max`，注明 `none` 仅为 OpenAI wire alias。
- Modify: `docs/design/02-modules/provider/01-domain-model-and-acl.md`
  - 记录 budget/effort 正交与 per-driver 显式 supported 集合要求。
- Modify: `docs/design/03-engineering/03-migration-governance.md`
  - 更新 P8/P11 当前落地说明并关联 #1393。

---

### Task 1: Shared Kernel 表达 `minimal` 与 `none` alias

**Files:**
- Modify: `agent/shared/src/reasoning.rs`
- Create: `agent/shared/src/reasoning_tests.rs`
- Modify: `agent/features/provider/src/ports.rs`

- [ ] **Step 1: 写失败测试**

在 `reasoning_tests.rs` 建立表驱动断言：

```rust
use super::ReasoningLevel;

#[test]
fn reasoning_level_parses_canonical_levels_and_openai_none_alias() {
    for level in [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ] {
        assert_eq!(ReasoningLevel::parse(level.as_str()), Some(level));
    }
    assert_eq!(ReasoningLevel::parse("none"), Some(ReasoningLevel::Off));
    assert_eq!(ReasoningLevel::Off.as_str(), "off");
}

#[test]
fn minimal_is_ordered_between_off_and_low() {
    assert!(ReasoningLevel::Off < ReasoningLevel::Minimal);
    assert!(ReasoningLevel::Minimal < ReasoningLevel::Low);
}
```

同步扩展 `ports.rs` 的 façade 断言，覆盖 `minimal` parse/display/serde 与 `none -> Off`。

- [ ] **Step 2: 运行测试并确认因缺少 `Minimal` 失败**

Run: `cargo test -p share reasoning`

Expected: FAIL，错误指出 `ReasoningLevel::Minimal` 不存在。

- [ ] **Step 3: 实现最小 Shared Kernel 变更**

在 `Off` 与 `Low` 之间增加 `Minimal`；`as_str()` 返回 `minimal`；`parse()` 同时接受 `minimal` 和 `none`。将 `reasoning.rs` 的内联测试迁到同级测试文件并通过 `#[cfg(test)] #[path = "reasoning_tests.rs"] mod tests;` 引入。

- [ ] **Step 4: 验证 Shared Kernel 与 Provider façade**

Run: `cargo test -p share reasoning && cargo test -p provider ports::tests`

Expected: PASS，且 `none` 只作为 alias，不改变 `Off` 的序列化 canonical value。

- [ ] **Step 5: 提交**

```bash
git add agent/shared/src/reasoning.rs agent/shared/src/reasoning_tests.rs agent/features/provider/src/ports.rs
git commit -m "feat(#1393): add minimal reasoning level"
```

---

### Task 2: 解除 legacy client 的 token-budget-to-effort 推导

**Files:**
- Modify: `agent/features/provider/src/adapters/client.rs`
- Test: 为该私有 resolver 创建/使用同级 `client_tests.rs`，禁止新增内联测试

- [ ] **Step 1: 写失败的 resolver 测试**

覆盖以下矩阵：

```rust
assert_eq!(reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(0))), ReasoningLevel::Off);
assert_eq!(reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(1))), ReasoningLevel::High);
assert_eq!(reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(40_000))), ReasoningLevel::High);
```

关键不变量：不同正数 budget 不再产生 Low/Medium/High/Xhigh 差异；正预算只表达启用，fallback level 保持当前 bool/legacy 默认 `High`。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider reasoning_level_from_options`

Expected: FAIL；当前 1 token 映射 Low，40,000 tokens 映射 Xhigh。

- [ ] **Step 3: 实现二值 budget 解析**

将 `ThinkingBudget` 分支改为仅判断 `0 => Off`、正数 `=> High`，不调用任何 token-to-effort helper。

- [ ] **Step 4: 运行定向测试**

Run: `cargo test -p provider reasoning_level_from_options`

Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add agent/features/provider/src/adapters/client.rs agent/features/provider/src/adapters/client_tests.rs
git commit -m "fix(#1393): decouple reasoning budget from effort level"
```

---

### Task 3: 重构 driver capability 与 wire effort 投影

**Files:**
- Modify: `agent/features/provider/src/adapters/openai_compatible/driver.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/clamp_effort.rs`

- [ ] **Step 1: 修改 OpenAI 与跨 driver 回归测试形成 RED**

将 OpenAI 断言改为：

```rust
let driver = OpenAiDriver;
for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
    assert_eq!(driver.clamp_effort(level), level);
}
assert_eq!(driver.reasoning_capability().maximum(), ReasoningLevel::Max);
```

新增表驱动快照，锁定其他 driver 的 maximum/mapping：Zhipu Max/Effort、LiteLLM Max/Effort、Volcengine Medium/Effort、MiniMax Medium/ThinkingToggle、Mimo Medium/ThinkingToggle、DeepSeek Max/Effort、Agnes Medium/ThinkingToggle；同时断言这些 driver 不因新增 `Minimal` 自动把它加入 supported 集合。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider openai_compatible::tests::test_clamp_effort`

Expected: FAIL；OpenAI 当前把 `xhigh/max` 降到 `high`，且 `minimal/none` 尚无正确投影。

- [ ] **Step 3: 实现显式 capability 与 wire mapper**

用接受显式 level iterator 的 helper 构造 capability，避免枚举扩展自动污染 driver 集合。为 `ChatApiDriver` 增加：

```rust
fn wire_effort(&self, level: ReasoningLevel) -> &'static str {
    level.as_str()
}
```

OpenAI 覆写 `Off => "none"`，其他 level 使用 `as_str()`；OpenAI capability 显式包含七档，其他 driver 明确保持原集合。删除 `effort_from_thinking_tokens`。

- [ ] **Step 4: 验证全 driver capability 矩阵**

Run: `cargo test -p provider openai_compatible::driver && cargo test -p provider openai_compatible::tests::test_clamp_effort`

Expected: PASS；只有 OpenAI maximum 从 High 变为 Max。

- [ ] **Step 5: 提交**

```bash
git add agent/features/provider/src/adapters/openai_compatible/driver.rs agent/features/provider/src/adapters/openai_compatible/tests/clamp_effort.rs
git commit -m "feat(#1393): expand openai effort capability"
```

---

### Task 4: 让 ThinkingBudget 不再生成 Chat effort

**Files:**
- Modify: `agent/features/provider/src/adapters/openai_compatible/reasoning.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/reasoning.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests.rs`

- [ ] **Step 1: 将现有 budget 映射测试改成失败的“不发送 effort”契约**

至少覆盖 OpenAI 与 LiteLLM：

```rust
#[test]
fn openai_thinking_budget_does_not_generate_effort() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();
    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn litellm_thinking_budget_does_not_generate_effort() {
    let config = ReasoningConfig::ThinkingBudget(40_000);
    let mut body = base_body();
    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);
    assert!(body.get("reasoning_effort").is_none());
}
```

保留 Volcengine/MiniMax/Mimo/Agnes 的 budget-as-enabled 测试，证明本次只解除 effort 推导，没有删除 toggle 语义。

- [ ] **Step 2: 运行测试并确认 RED**

Run: `cargo test -p provider thinking_budget`

Expected: FAIL；OpenAI/LiteLLM 当前仍从预算生成 effort。

- [ ] **Step 3: 删除所有 budget-to-effort 路径**

- `ReasoningConfig::as_effort()` 对 `ThinkingBudget` 返回 `None`。
- `ReasoningConfig::for_scope()` 对 `ThinkingBudget` 保持 `ThinkingBudget(tokens)`，不得改写成 Object effort。
- `ReasoningConfig::clamped()` 对 `ThinkingBudget` 保持原值。
- OpenAI `apply_reasoning_fields` 不再处理 ThinkingBudget。
- 删除 helper re-export 和边界测试。

把 `tests.rs` 中 `include!("tests/*.rs")` 改成显式 path modules，使测试文件拥有正常模块边界；迁移时不改变无关断言。

- [ ] **Step 4: 验证 Chat 及其他 driver**

Run: `cargo test -p provider openai_compatible::tests`

Expected: PASS；OpenAI/LiteLLM budget 不产生 effort，toggle driver 原行为保持。

- [ ] **Step 5: 提交**

```bash
git add agent/features/provider/src/adapters/openai_compatible.rs agent/features/provider/src/adapters/openai_compatible/reasoning.rs agent/features/provider/src/adapters/openai_compatible/tests.rs agent/features/provider/src/adapters/openai_compatible/tests/
git commit -m "fix(#1393): stop mapping token budget to effort"
```

---

### Task 5: 对齐 OpenAI Chat 与 Responses 全档位映射

**Files:**
- Modify: `agent/features/provider/src/adapters/openai_compatible/responses.rs`
- Create: `agent/features/provider/src/adapters/openai_compatible/tests/responses_reasoning.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/reasoning.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests.rs`

- [ ] **Step 1: 写 Chat/Responses 对称失败测试**

对 `Minimal/Low/Medium/High/Xhigh/Max` 逐项构造 `InvocationScope`，断言两条路径都发送相同字符串；对 `Off` 断言 Chat 与 Responses 都不发送 reasoning 字段。Responses 重点断言：

```rust
for (level, expected) in [
    (ReasoningLevel::Minimal, "minimal"),
    (ReasoningLevel::Low, "low"),
    (ReasoningLevel::Medium, "medium"),
    (ReasoningLevel::High, "high"),
    (ReasoningLevel::Xhigh, "xhigh"),
    (ReasoningLevel::Max, "max"),
] {
    let body = provider.build_responses_request_body(&scope(level), &[], &[], &[], false);
    assert_eq!(body["reasoning"]["effort"], expected);
}
```

另对 driver wire mapper 断言 `OpenAI Off -> none`，但请求层仍因 Off 禁用而省略字段。

- [ ] **Step 2: 运行测试并确认 RED**

Run: `cargo test -p provider responses_reasoning`

Expected: FAIL；至少 `Minimal` 尚未编译或 Responses 尚未统一调用 wire mapper。

- [ ] **Step 3: 统一请求投影**

Responses 改用 `self.driver.wire_effort(scope.effective_reasoning())`；Chat 的 `ReasoningConfig::from_scope/for_scope` 同样使用该 mapper。保持 Off 的请求省略规则，避免把禁用态误编码成启用 reasoning。

- [ ] **Step 4: 验证 Chat/Responses 契约**

Run: `cargo test -p provider openai_effort && cargo test -p provider responses_reasoning`

Expected: PASS，六个启用档位在两条 API style 中一致。

- [ ] **Step 5: 提交**

```bash
git add agent/features/provider/src/adapters/openai_compatible/responses.rs agent/features/provider/src/adapters/openai_compatible/reasoning.rs agent/features/provider/src/adapters/openai_compatible/tests.rs agent/features/provider/src/adapters/openai_compatible/tests/reasoning.rs agent/features/provider/src/adapters/openai_compatible/tests/responses_reasoning.rs
git commit -m "fix(#1393): align openai chat and responses effort"
```

---

### Task 6: 更新 capability resolver 与非 OpenAI 回归契约

**Files:**
- Modify: `agent/features/provider/src/published_language.rs`
- Modify: `agent/features/provider/src/adapters/openai_compatible/tests/provider_config.rs`

- [ ] **Step 1: 写 capability resolver 测试**

增加两个互补断言：

```rust
let openai = ReasoningCapability::new(
    [ReasoningLevel::Off, ReasoningLevel::Minimal, ReasoningLevel::Low, ReasoningLevel::Medium, ReasoningLevel::High, ReasoningLevel::Xhigh, ReasoningLevel::Max],
    ReasoningMappingKind::Effort,
).unwrap();
assert_eq!(openai.resolve(ReasoningLevel::Minimal), ReasoningLevel::Minimal);
assert_eq!(openai.resolve(ReasoningLevel::Max), ReasoningLevel::Max);

let legacy = ReasoningCapability::new(
    [ReasoningLevel::Off, ReasoningLevel::Low, ReasoningLevel::Medium],
    ReasoningMappingKind::Effort,
).unwrap();
assert_eq!(legacy.resolve(ReasoningLevel::Minimal), ReasoningLevel::Off);
```

- [ ] **Step 2: 运行 resolver 与 driver contract**

Run: `cargo test -p provider resolver_selects && cargo test -p provider provider_config`

Expected: 新 resolver 测试在实现前 FAIL；实现后所有非 OpenAI wire 测试继续 PASS。

- [ ] **Step 3: 仅更新测试矩阵，不改 resolver 算法**

`ReasoningCapability::resolve` 已按有序 supported 集合向下选择，不需要生产逻辑变化。若测试暴露 helper 自动包含 `Minimal`，回到 Task 3 修正显式集合，禁止在 resolver 中加 driver 特例。

- [ ] **Step 4: 运行 Provider 全量测试**

Run: `cargo test -p provider`

Expected: PASS；基线 216 个有效测试（当前 214 unit + 2 integration）全部保留，并包含新增回归。

- [ ] **Step 5: 提交**

```bash
git add agent/features/provider/src/published_language.rs agent/features/provider/src/adapters/openai_compatible/tests/provider_config.rs
git commit -m "test(#1393): lock reasoning driver contracts"
```

---

### Task 7: 同步 Target 文档与配置说明

**Files:**
- Modify: `agent/shared/src/config/domain/models/types.rs`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/02-modules/provider/01-domain-model-and-acl.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1: 更新 Shared Kernel 与配置词汇**

配置注释列出 `off/none/minimal/low/medium/high/xhigh/max`，明确：`none` 解析为领域 `Off`；canonical config/display 仍为 `off`；`minimal` 是独立有序档位。

- [ ] **Step 2: 更新 Provider 映射规则**

在 Provider Target 文档记录：

- `ReasoningLevel = Off / Minimal / Low / Medium / High / Xhigh / Max`；
- budget 与 effort 是正交协议维度，禁止按 token 阈值推导 effort；
- OpenAI wire 把 `Off` 命名为 `none`，但禁用请求通常省略 reasoning 字段；
- 每个 driver 必须显式声明 supported 集合，共享枚举扩展不得自动扩大 driver capability。

- [ ] **Step 3: 更新迁移治理状态**

在 P8/P11 当前落地中关联 #1393：OpenAI 完整 effort 档位、budget 解耦、其他 driver 契约回归；不要声称 #1142 的生产 resolver 接线已完成。

- [ ] **Step 4: 检查术语一致性**

Run: `rg 'Off / Low|off"/"low|effort_from_thinking_tokens|token.*映射.*effort' agent/shared docs/design/02-modules/provider docs/design/03-engineering/03-migration-governance.md`

Expected: 不再存在与新七档 Shared Kernel 或 budget 正交原则冲突的活跃描述；历史记录可保留但必须有时间/issue 上下文。

- [ ] **Step 5: 提交**

```bash
git add agent/shared/src/config/domain/models/types.rs docs/design/01-system/03-context-map.md docs/design/02-modules/provider/01-domain-model-and-acl.md docs/design/03-engineering/03-migration-governance.md
git commit -m "docs(#1393): define reasoning effort mapping semantics"
```

---

### Task 8: 完整验证与清理

**Files:**
- Verify all modified files

- [ ] **Step 1: 格式化检查**

Run: `cargo fmt --all --check`

Expected: PASS。若失败，运行 `cargo fmt --all` 后重新执行 `--check`。

- [ ] **Step 2: 运行受影响 crate 测试**

Run: `cargo test -p share && cargo test -p provider`

Expected: PASS，0 failures。

- [ ] **Step 3: 运行相邻消费方测试**

Run: `cargo test -p runtime -p composition`

Expected: PASS，证明新增 `Minimal` 没有破坏 Runtime/Composition 的 exhaustive match、scope 翻译与 clamp。

- [ ] **Step 4: 运行编译与 lint 门禁**

Run: `cargo check --workspace && cargo clippy -p share -p provider -p runtime -p composition --all-targets -- -D warnings`

Expected: PASS，0 errors/warnings。

- [ ] **Step 5: 运行 Provider 架构守卫**

Run: `bash .agents/hooks/check-provider-usage-capability.sh && bash .agents/hooks/check-no-inline-tests.sh`

Expected: PASS；capability/legacy clamp 仍由唯一声明派生，没有新内联测试。

- [ ] **Step 6: 检查死代码、范围与 issue 验收项**

Run:

```bash
rg 'effort_from_thinking_tokens|ThinkingBudget\([^)]*\).*effort' agent/features/provider/src
git diff --check
git status --short
git diff origin/main...HEAD --stat
```

Expected: helper 与 token-budget-to-effort 路径无匹配；diff 无 whitespace error；只包含 #1393 范围文件。

- [ ] **Step 7: 更新 Issue 门禁状态**

根据实际验证证据编辑 #1393：完成的 checklist 勾选；L4/L5 保持 N/A 并记录理由；附上精确测试与守卫命令结果。不要关闭 issue。

- [ ] **Step 8: 最终提交（仅在验证产生格式/文档修正时）**

```bash
git add <本步骤实际修正的文件>
git commit -m "chore(#1393): finalize reasoning mapping verification"
```

如果工作树无新增变更则跳过，不创建空提交。
