# #1438 Skill 特殊动态 Tool 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> 对应 Issue：[#1438](https://github.com/rushsinging/aemeath/issues/1438)｜Milestone：`v0.1.0 — Context Engineering + 架构重构`

**Goal:** 将 Skill 恢复为由 LLM 按名称调用、在调用时动态加载正文的特殊 Tool；Runtime 只向 TUI/LLM 发布 Skill 元数据，TUI 只发送统一 Skill 请求事件，Skill 正文不再被启动期复制或 Context 全量预注入。

**Architecture:** Tools BC 保留唯一的 Skill Catalog，并将“全量 PromptFragment 物化”替换为“按 identity 加载单个 Skill”的 `SkillLoadPort`；稳定注册的 `Skill` Tool schema 只有 `skill: string`，调用时结合 live workspace 与 Run 冻结配置/工具集合查询正文。Command Router 只负责把 Skill 的 slash 入口分类成 `SkillRequest`，TUI/no-TUI 发同一个 SDK 事件，Runtime 再把它投影成模型可见的用户意图；LLM 自行调用 `Skill` Tool。普通 Command 继续按 SnapshotQuery/ApplicationControl 确定性执行。

**Tech Stack:** Rust、Tokio、serde/serde_yml、Tool Catalog/Execution 双端口、ContextPromptSource、SDK typed events、ratatui/TEA、Cargo tests、shell architecture guards。

---

## 已锁定语义

1. **Skill 是特殊动态 Tool。** 只有一个稳定的 `Skill` Tool；具体 Skill 不是独立 Tool schema。
2. **Tool 输入只有 identity。** 模型调用形状固定为 `{"skill":"release"}`；用户输入的原始参数不进入 Tool input。
3. **参数是 LLM 参考上下文。** `/release v1.2.3` 产生统一 `SkillRequest { skill, arguments }` 事件，Runtime 将两者放入模型可见请求；LLM 加载正文后自行结合参数理解。
4. **正文调用时加载。** Run Step 目录只冻结元数据；Tool 调用时重新读取 live workspace 下的文件。目录后删除/失效返回 typed failure，不使 Run 崩溃。
5. **Skill 与 Command 不同。** Skill slash 入口只生成模型请求；`/clear`、`/help`、`/compact` 等 Command 仍由确定性 handler 执行，不进入 LLM，也不调用 `Skill` Tool。
6. **不增加第二套可见性协议。** Skill 正文使用正常 ToolResult 文本进入 LLM；TUI 通过 `ToolDisplayEntry` 隐藏 Skill result，只显示 `Skill(name)` header。
7. **根因级切线，不保留兼容双路径。** 删除 TUI 正文副本、Context 全文 Skill block、`PromptInjection` Skill 执行语义及“Skill 不是 Tool”测试/Guard；不以别名或兼容 adapter 留下第二条正文交付路径。

## 文件结构

| 文件 | 职责 / 改动 |
|---|---|
| `agent/features/tools/src/domain/skill_pl.rs` | 将 PromptFragment/全量物化 DTO 收窄为 Skill metadata、`SkillLoadQuery`、`LoadedSkill` 与 typed load error。 |
| `agent/features/tools/src/domain/skill_ports.rs` | 保留 `SkillCatalogPort`，以按 identity 的 `SkillLoadPort` 替代 `SkillMaterializationPort`。 |
| `agent/features/tools/src/adapters/skill_filesystem.rs` | 复用唯一发现/优先级逻辑，实现元数据 list 与调用时单 Skill load。 |
| `agent/features/tools/src/adapters/skill_tool.rs` | 新增稳定 `Skill` TypedTool；schema 仅含 `skill`，正文作为 ToolResult text。 |
| `agent/features/tools/src/adapters/{registry.rs,composition.rs}` | Main/Sub 注册 Skill；Composition 用同一 filesystem adapter 装配 Catalog、Load Port 与 Skill Tool。 |
| `agent/features/tools/src/domain/context.rs` | 为 ToolExecutionContext 增加窄的 per-Run `SkillQuerySnapshot` 值，不加入 ExecutionScope 八字段。 |
| `agent/features/context/src/adapters/skill_prompt_source.rs` | 从 Skill Catalog 元数据生成受预算约束目录，不读取正文。 |
| `agent/features/context/src/{ports.rs,adapters.rs}`、`adapters/canonical_session.rs` | 将 prompt supplier 从 materializer 改为 catalog + metadata query factory。 |
| `packages/sdk/src/{chat.rs,tui.rs}` | 新增统一 `ChatInputEvent::SkillRequest`；`SkillView` 删除正文/source，增加 `argument_hint`。 |
| `agent/features/tools/src/domain/{command_pl.rs,command_ports.rs}`、`adapters/command.rs` | 将 Skill slash 路由显式分类为 `SkillRequest`，不再借用 PromptInjection。 |
| `agent/composition/src/tools.rs` | Skill metadata 只投影 slash descriptor/route，不携带正文。 |
| `agent/composition/src/runtime.rs` | 一次装配同一个 Skill filesystem backing，并把 Catalog/Load Port 分发给 Context 与 Tool factory。 |
| `agent/features/runtime/src/application/{runtime_context.rs,runtime_context_factory.rs}` | 将冻结的 Skill 查询值装配进 Main/Sub ToolExecutionContext。 |
| `agent/features/runtime/src/application/main_loop/looping/{input_gate.rs,run_input_buffer.rs,main_run_port.rs}` | 接纳统一 SkillRequest，保留 InputId/原始参数并投影为模型可见用户消息。 |
| `agent/features/runtime/src/application/subagent/runner/{setup.rs,loop_run.rs}` | Sub-agent 同样获得 Skill Tool 与冻结查询快照。 |
| `apps/cli/src/tui/app/{slash.rs,run_loop.rs,runtime.rs}` | Skill slash 只发送 typed event；移除正文 lookup/拼接/回显。 |
| `apps/cli/src/chat/no_tui.rs` | no-TUI 使用同一 Router 和 SkillRequest 事件语义。 |
| `apps/cli/src/tui/render/output/tool_display/tool_impls/skill.rs` | 新增 Skill header，并隐藏 result 正文。 |
| `.agents/hooks/check-tool-catalog-execution-boundary{,-tests}.sh` | 删除禁止新 Skill Tool 的名称黑名单，改为锁定唯一动态 Skill Tool 与无旧 DTO/旁路。 |
| `docs/design/**`、`specs/{runtime.md,tools.md,tui-cli.md,prompt.md}` | 同步新术语、端口、快照、事件和 Guard 真相。 |

## Task 1：先修正 Target 文档与术语冲突

**Files:**
- Modify: `docs/design/01-system/02-ubiquitous-language.md`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/02-modules/tools/01-domain-model.md`
- Modify: `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- Modify: `docs/design/02-modules/context-management/04-prompt-guidance.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`

- [ ] **Step 1: 建立开发前差异清单**

在 #1438 Issue comment 中逐项记录当前不符合点：`Skill 不是 Tool`、`PromptFragment` 全文注入、`PromptInjection` slash、`SkillView.content`、`skill_not_tool_contract_tests`、Guard 对 `SkillTool` 的绝对禁止。不得在差异清单完成前改生产代码。

- [ ] **Step 2: 修改目标领域模型**

将统一术语锁定为：

```text
SkillDescriptor        可发现的廉价元数据
SkillRequest           用户请求 LLM 使用 Skill（name + raw reference arguments）
SkillLoadPort          调用时按 identity 加载一个 Skill
LoadedSkill            name + content + source + content revision
Skill Tool              LLM 可调用的稳定特殊 Tool，input 只有 skill
Command                 应用确定性查询/控制；不执行 Skill
```

明确 `SkillLoadPort` 属于 Tools BC，Context 只消费 `SkillCatalogPort`，Runtime 只编排 SkillRequest 与 ToolExecution，不读取 Skill 文件。

- [ ] **Step 3: 更新测试证据矩阵**

在 `04-testing-and-coverage.md` 将旧“materialization 到 Context prompt block”行改为：Tools L2/L3 证明 metadata/load；Context L2/L3 证明 metadata directory；SDK/Runtime/TUI L3/L4 证明 SkillRequest 字段不丢失；Tool 场景证明正文仅在调用后进入模型上下文。

- [ ] **Step 4: 文档自检并提交**

Run:

```bash
rg -n 'Skill 不是 Tool|Skill is NOT a Tool|SkillTool|PromptFragment|PromptInjection' \
  docs/design specs agent/features/tools/src/adapters/skill_not_tool_contract_tests.rs
```

Expected: 只剩明确标注为 Current/待迁移的差异和历史 changelog；Target 正文不再宣称 Skill 永不进入 Tool Catalog。

Commit:

```bash
git add docs/design
git commit -m "docs: redefine skill as dynamic tool"
```

## Task 2：以按 identity 加载替换全量 PromptFragment 物化

**Files:**
- Modify: `agent/features/tools/src/domain/skill_pl.rs`
- Modify: `agent/features/tools/src/domain/skill_ports.rs`
- Modify: `agent/features/tools/src/domain/skill_pl_tests.rs`
- Modify: `agent/features/tools/src/adapters/skill_filesystem.rs`
- Modify: `agent/features/tools/src/adapters/skill_filesystem_tests.rs`
- Modify: `agent/features/tools/src/domain.rs`
- Modify: `agent/features/tools/src/lib.rs`

- [ ] **Step 1: 写失败的 Skill PL 测试**

新增断言：

```rust
let query = SkillLoadQuery::new(
    "release",
    project_root.clone(),
    extra_dirs.clone(),
    available_tools.clone(),
);
assert_eq!(query.identity(), "release");

let loaded = LoadedSkill::new("release", "body", source, "revision");
assert_eq!(loaded.content(), "body");
```

覆盖空 identity 被拒绝、identity alias 规范化、`SkillError::NotFound { identity }` 与读取/解析错误分类；`SkillDescriptor` 增加 `argument_hint: Option<String>`，但不得增加 content。

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test -p tools domain::skill_pl -- --nocapture
```

Expected: FAIL，原因是 `SkillLoadQuery`、`LoadedSkill`、`NotFound` 和 `argument_hint` 尚不存在。

- [ ] **Step 3: 定义窄端口**

将端口收敛为：

```rust
pub trait SkillCatalogPort: Send + Sync {
    fn list(&self, query: SkillQuery) -> Vec<SkillDescriptor>;
}

#[async_trait]
pub trait SkillLoadPort: Send + Sync {
    async fn load(&self, query: SkillLoadQuery) -> Result<LoadedSkill, SkillError>;
}
```

删除 `PromptFragment`、`CacheHint`、`SkillMaterializationQuery`、`SkillMaterializationSnapshot`、`SkillMaterializationRevision` 与 `SkillMaterializationPort` 的生产导出；不得保留 deprecated alias。

- [ ] **Step 4: 写 filesystem 调用时加载失败测试**

在每测试唯一临时目录中：先 `list` 得到 `release`，随后删除 `release/SKILL.md`，再执行 `load("release")`，断言得到 `SkillError::NotFound`。另覆盖 name、identity alias、来源优先级、`requires_tools`/`fallback_for` 过滤、损坏目标文件 typed error、未命中时不返回其他 Skill 正文。

- [ ] **Step 5: 实现单 Skill load 并复用发现逻辑**

`FilesystemSkillAdapter::load` 每次调用 `collect_available`，按 canonical name 或 identity alias 精确选择一个 `RawSkill`，再转成 `LoadedSkill`；不得先生成全量正文快照，也不得缓存启动期文件内容。`argument-hint` 仅从 frontmatter 进入 descriptor。

- [ ] **Step 6: 运行 Tools 定向测试并提交**

Run:

```bash
cargo test -p tools skill_pl -- --nocapture
cargo test -p tools skill_filesystem -- --nocapture
```

Expected: PASS；删除竞态稳定返回 NotFound，目录不含 content。

Commit:

```bash
git add agent/features/tools/src/domain agent/features/tools/src/adapters/skill_filesystem.rs agent/features/tools/src/adapters/skill_filesystem_tests.rs agent/features/tools/src/lib.rs
git commit -m "refactor: load skills by identity"
```

## Task 3：Context 只生成 Run Step 级 Skill 元数据目录

**Files:**
- Modify: `agent/features/context/src/adapters/skill_prompt_source.rs`
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/src/adapters.rs`
- Modify: `agent/features/context/src/ports.rs`
- Modify: `agent/features/context/tests/skill_prompt_pipeline.rs`
- Modify: `agent/features/context/tests/isolated_context_with_skill.rs`
- Modify: `agent/composition/src/runtime.rs`
- Test: `agent/composition/tests/main_session_wiring.rs`

- [ ] **Step 1: 将现有 Context 测试改成失败的 metadata 断言**

使用 fake `SkillCatalogPort` 返回 descriptor，断言 `skills` system block：

```text
# Available Skills
- release: create a release
  Usage: /release [version]
```

并断言正文 sentinel `FULL_SKILL_BODY_MUST_NOT_APPEAR` 永不进入任何 system block。覆盖中英文 header、stable name 排序、重复 identity 去重、description 截断/总预算、空目录省略 block。

- [ ] **Step 2: 运行测试确认旧全文实现失败**

Run:

```bash
cargo test -p context skill_prompt_pipeline -- --nocapture
cargo test -p context --test isolated_context_with_skill -- --nocapture
```

Expected: FAIL，旧 pipeline 仍要求 `PromptFragment` 并渲染正文。

- [ ] **Step 3: 将 query factory 改成 metadata query**

`SkillQueryFactory` 返回 `SkillQuery`；`SkillPromptSource` 持 `Arc<dyn SkillCatalogPort>`，在每次 `ContextRequest`（即 Run Step freeze 后）用 live workspace root、Run config 的 `skills.dirs`、当前 `tool_schemas` 构造查询。

- [ ] **Step 4: 实现确定性目录预算**

保留 Context 对 system block 顺序、预算与 cacheability 的所有权，但 token 成本只计算格式化 metadata。revision 由 descriptor 字段（name/description/slash/argument_hint/source identity）确定性计算，不读取正文 revision。

- [ ] **Step 5: 修改 Main/Sub Context 装配**

`ProductionMainContextFactory::with_skill_catalog` 和 `isolated_context_with_skill_catalog` 只接收 Catalog；Composition 将 `skill_wiring.catalog()` 传入 Main，Sub runner 复用同一个 port。删除所有 `with_skill_supplier`/materializer 字段及 accessor。

- [ ] **Step 6: 运行 Context/Composition 测试并提交**

Run:

```bash
cargo test -p context skill -- --nocapture
cargo test -p composition --test main_session_wiring -- --nocapture
```

Expected: PASS；每个 ContextRequest 得到元数据目录，system prompt 不含任一 Skill 正文。

Commit:

```bash
git add agent/features/context agent/composition/src/runtime.rs agent/composition/tests/main_session_wiring.rs
git commit -m "refactor: expose skill metadata to context"
```

## Task 4：注册稳定 Skill Tool 并完成调用时动态加载

**Files:**
- Create: `agent/features/tools/src/adapters/skill_tool.rs`
- Create: `agent/features/tools/src/adapters/skill_tool_tests.rs`
- Modify: `agent/features/tools/src/adapters.rs`
- Modify: `agent/features/tools/src/adapters/registry.rs`
- Modify: `agent/features/tools/src/adapters/composition.rs`
- Modify: `agent/features/tools/src/domain/context.rs`
- Modify: `agent/features/tools/src/domain/context_tests.rs`
- Modify: `agent/features/tools/src/domain/published_language.rs`
- Modify: `agent/features/tools/src/lib.rs`
- Modify: `agent/composition/src/runtime.rs`
- Modify: `agent/features/runtime/src/application/runtime_context.rs`
- Modify: `agent/features/runtime/src/application/runtime_context_factory.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/{setup.rs,loop_run.rs}`
- Replace: `agent/features/tools/src/adapters/skill_not_tool_contract_tests.rs` → `skill_tool_contract_tests.rs`

- [ ] **Step 1: 将反向契约改成失败的正向契约**

删除“Skill 必须 ToolUnavailable”的断言，改为 Main/Sub catalog 均包含 canonical `Skill`，input schema 精确为：

```json
{
  "type": "object",
  "properties": {
    "skill": { "type": "string", "minLength": 1 }
  },
  "required": ["skill"],
  "additionalProperties": false
}
```

显式断言 schema 不含 `args`、`arguments`、`content`。执行成功时 ToolOutcome text 等于目标 Skill 正文；目标在 catalog snapshot 后被删除时返回 failure 且不 panic。

- [ ] **Step 2: 运行契约测试确认失败**

Run:

```bash
cargo test -p tools skill_tool_contract -- --nocapture
```

Expected: FAIL，当前 Main/Sub catalog 都没有 Skill。

- [ ] **Step 3: 定义 per-Run `SkillQuerySnapshot`**

在 `domain/context.rs` 增加只含纯值的：

```rust
#[derive(Clone)]
pub struct SkillQuerySnapshot {
    pub extra_dirs: Vec<PathBuf>,
    pub available_tools: BTreeSet<String>,
}
```

`project_root` 不复制进该值，Skill Tool 调用时从 `ctx.workspace_read().current_workspace_root()` 读取 live root；`extra_dirs` 和 `available_tools` 来自所属 Run 的冻结 config/catalog。通过 `ToolExecutionPorts::with_skill_query` 注入，并由 `ToolExecutionContext::skill_query()` 只读取得。不得扩展固定八字段 `ExecutionScope`。

- [ ] **Step 4: 实现 `SkillTool`**

`SkillTool` 持 `Arc<dyn SkillLoadPort>`，只解析 `skill`。调用时组合 live project root 与 `ctx.skill_query()`，执行 `load`；成功返回：

```rust
TypedToolResult::success(
    loaded.content().to_string(),
    SkillLoadResult { name: loaded.name().to_string(), revision: loaded.revision().to_string() },
)
```

NotFound/ReadFailed/ParseFailed 映射为安全中文 typed failure；正文不得放入 `data`，避免展示层结构化 payload 再持有副本。Tool 声明 read-only、concurrency-safe、cooperative cancellation；input safety 设为 Always。

- [ ] **Step 5: 由 Composition 用同一 Skill backing 装配 Tool**

扩展 `wire_builtin_catalog_execution` 的构造输入，接收 `Arc<dyn SkillLoadPort>`，Main/Sub registry 都以只读 capability 注册 Skill。`wire_skills()` 返回同一 filesystem adapter 的 catalog/load 两个 façade；`agent/composition/src/runtime.rs` 只构造一次 `SkillWiring`，先把 load port 传给 Tool factory，再把 catalog 传给 Context/Runtime dependencies。

- [ ] **Step 6: Main/Sub 注入冻结查询值**

Main 在 Run/Step tool schema 已冻结后构造 `SkillQuerySnapshot`；Sub 在派生 RuntimeContext 时从 Sub catalog/profile 与 Sub RunConfigSnapshot 独立构造，不从 Main 复制可用工具集合。更新所有 `ToolExecutionPorts::new` fixture，测试用显式 empty/default snapshot，禁止增加进程全局配置读取。

- [ ] **Step 7: 运行 Tool/Runtime/Composition 测试并提交**

Run:

```bash
cargo test -p tools skill_tool -- --nocapture
cargo test -p tools catalog_execution -- --nocapture
cargo test -p runtime runtime_context -- --nocapture
cargo test -p composition runtime -- --nocapture
```

Expected: PASS；Main/Sub 都能调用 Skill，且不同 Run 使用各自冻结的 dirs/tool names 与调用时 live workspace root。

Commit:

```bash
git add agent/features/tools agent/features/runtime/src/application agent/composition/src/runtime.rs
git commit -m "feat: add dynamic skill tool"
```

## Task 5：建立 SDK SkillRequest Published Language

**Files:**
- Modify: `packages/sdk/src/chat.rs`
- Modify: `packages/sdk/src/tui.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `agent/features/tools/src/domain/command_pl.rs`
- Modify: `agent/features/tools/src/domain/command_pl_tests.rs`
- Modify: `agent/features/tools/src/adapters/command.rs`
- Modify: `agent/features/tools/src/adapters/command_tests.rs`
- Modify: `agent/features/tools/tests/command_contract.rs`
- Modify: `agent/composition/src/tools.rs`
- Modify: `agent/composition/tests/command_wiring.rs`

- [ ] **Step 1: 写失败的 SDK 事件测试**

新增纯值请求：

```rust
pub struct SkillRequest {
    pub input_id: InputId,
    pub skill: String,
    pub arguments: String,
}

pub enum ChatInputEvent {
    SkillRequest(SkillRequest),
    // existing variants...
}
```

断言 `/release v1.2.3` 的 name 与 raw arguments 独立保留；`PartialEq` 继续按 discriminant 的既有约定；事件不含 `content`、`source`、ToolInvocation 或 Tool 类型。

- [ ] **Step 2: 写失败的 Command/Skill 分类测试**

将 `CommandMechanism::PromptInjection` 重命名为 `SkillRequest`，将 route 变体定义为：

```rust
CommandRoute::SkillRequest(PromptCommand)
```

其中 `PromptCommand` 可同步重命名为 `SkillRequestCommand`，避免继续保留“prompt injection”语义。断言 Skill slash alias `/cr staged` 得到 canonical Skill identity + `staged`，而 `/clear` 仍得到 `ApplicationControl`，`/help` 仍得到 `SnapshotQuery`。

- [ ] **Step 3: 运行 SDK/Tools/Composition 测试确认失败**

Run:

```bash
cargo test -p sdk chat_input_event -- --nocapture
cargo test -p tools command -- --nocapture
cargo test -p composition --test command_wiring -- --nocapture
```

Expected: FAIL，当前 route 仍是 PromptInjection，SDK 没有 SkillRequest。

- [ ] **Step 4: 收窄 SkillView**

`SkillView` 只保留：

```rust
pub struct SkillView {
    pub name: String,
    pub aliases: Vec<String>,
    pub slash_command: Option<String>,
    pub slash_aliases: Vec<String>,
    pub description: String,
    pub argument_hint: Option<String>,
}
```

删除 `content` 和 `source`。Composition 的 slash descriptor 必须保存 canonical Skill identity；若当前 `CommandDescriptor.name` 只能保存 slash name，则给 Skill route descriptor 增加 `target_identity` 值字段，禁止 TUI 再通过自己的 HashMap alias 搜索还原 identity。

- [ ] **Step 5: 实现分类路由并提交**

Command Adapter 只解析名称/参数并返回 typed route，不读 Skill、不触发 Tool、不生成 LLM 文本。完成后运行：

```bash
cargo test -p sdk --lib -- --nocapture
cargo test -p tools command -- --nocapture
cargo test -p composition command_wiring -- --nocapture
```

Expected: PASS；Skill 和 Command 的机制/target 不混用。

Commit:

```bash
git add packages/sdk agent/features/tools/src/domain/command_pl.rs agent/features/tools/src/domain/command_pl_tests.rs agent/features/tools/src/adapters/command.rs agent/features/tools/src/adapters/command_tests.rs agent/features/tools/tests/command_contract.rs agent/composition
git commit -m "feat: publish typed skill requests"
```

## Task 6：Runtime 将 SkillRequest 投影为模型可见用户意图

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/input.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/input_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/input_gate.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/input_gate_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/run_input_buffer.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/run_input_buffer_tests.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`

- [ ] **Step 1: 写失败的模型投影测试**

新增唯一格式化函数，按 `Config.language` 生成结构化请求；中文示例：

```text
<skill-request>
用户请求使用 Skill：release
参考参数：v1.2.3
请先调用 Skill 工具加载该 Skill，再结合参考参数理解并执行。
</skill-request>
```

英文提供同义模板。断言原始参数只出现一次，不插入 SKILL.md，不伪造 Tool Call；空参数省略“参考参数”行。

- [ ] **Step 2: 写 busy/admission/withdraw 失败测试**

覆盖：idle SkillRequest 成为一个带原 InputId 的 user message；busy 时进入 RunInputBuffer 并在下一个 Step 接纳；sealed 时与 UserMessage 相同地退回 pending；WithdrawAll 能返回用户原始 `/release v1.2.3` 文本而不是 XML 投影。为此 `SkillRequest` 必须保存 `raw_input` 或由 name/arguments 唯一重建，测试锁定用户可见回滚值。

- [ ] **Step 3: 运行 Runtime 测试确认失败**

Run:

```bash
cargo test -p runtime input_gate skill_request -- --nocapture
cargo test -p runtime run_input_buffer skill_request -- --nocapture
```

Expected: FAIL，现有 gate 不认识 SkillRequest。

- [ ] **Step 4: 使 SkillRequest 成为 user-run input 而非 control**

`split_input_events`、idle gate、busy queue、RunInputBuffer 的 user-input 计数/seal/withdraw/drain 都把 `SkillRequest` 与 `UserMessage` 放在同一 admission 类，但在构建 `share::Message` 时调用唯一 SkillRequest formatter。Command/control 分支不变，绝不把 `ControlCommand` 投影给 LLM。

- [ ] **Step 5: 验证 Step 快照和持久化**

`ContextRequest.pending_messages` 应包含格式化后的用户意图；accepted/finalized Step 保留同一 InputId 与消息 ownership。Session resume 后该用户意图可重建，但 Skill 正文只存在于配对 ToolResult 中。

- [ ] **Step 6: 运行 Runtime 测试并提交**

Run:

```bash
cargo test -p runtime loop_engine::input -- --nocapture
cargo test -p runtime input_gate -- --nocapture
cargo test -p runtime run_input_buffer -- --nocapture
cargo test -p runtime loop_runner -- --nocapture
```

Expected: PASS；idle/busy/sealed/withdraw 均无字段丢失。

Commit:

```bash
git add agent/features/runtime/src/application/loop_engine agent/features/runtime/src/application/main_loop/looping
git commit -m "feat: admit skill requests into runs"
```

## Task 7：TUI/no-TUI 只发送统一事件，不接触正文

**Files:**
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/adapters/tui_launch.rs`
- Modify: `apps/cli/src/tui/app/slash.rs`
- Modify: `apps/cli/src/tui/app/slash_tests.rs`
- Modify: `apps/cli/src/tui/app/run_loop.rs`
- Modify: `apps/cli/src/tui/app/run_loop_tests.rs`
- Modify: `apps/cli/src/tui/app/runtime.rs`
- Modify: `apps/cli/src/command_contract_tests.rs`
- Modify: `apps/cli/src/chat/no_tui.rs`
- Test: `apps/cli/src/chat/no_tui.rs` test module

- [ ] **Step 1: 写失败的 TUI route 测试**

给 App 注入只有 metadata 的 `SkillView`，调用 `/release v1.2.3`，断言产生：

```rust
ChatInputEvent::SkillRequest(SkillRequest {
    skill: "release".into(),
    arguments: "v1.2.3".into(),
    ..
})
```

并断言返回值不再是 `Some(skill_body)`、timeline 不出现正文、TUI 不调用 `find_skill_by_alias`。Command `/clear` 仍发送 Reset/本地执行，未知 slash 仍由 Router 报错。

- [ ] **Step 2: 写失败的 no-TUI 对称测试**

`run_single_turn` 遇到 Skill route 时创建 input event port 并发送同一 SkillRequest，而不是提示“不支持 no-TUI”或把 `/release ...` 当普通 UserInput。断言 TUI/no-TUI 解析结果与事件 payload 完全一致。

- [ ] **Step 3: 运行 CLI 测试确认失败**

Run:

```bash
cargo test -p cli command_contract -- --nocapture
cargo test -p cli slash skill -- --nocapture
cargo test -p cli no_tui -- --nocapture
```

Expected: FAIL，当前 TUI 读取 `SkillView.content` 并返回正文。

- [ ] **Step 4: 删除启动期正文复制**

`from_args.rs` 只调用 `skill_catalog.list` 生成 `skills_map`；不得调用 load port，不得建立 fragments map。删除 `SkillView.content/source` 的全部构造与测试 fixture；`find_skill_by_alias` 若不再有补全消费者则删除，补全只消费 Command Catalog。

- [ ] **Step 5: TUI 直接发送 event effect**

将 `handle_slash_command_with_events` 的 Skill 分支改为 `Effect::SendChatInputEvent` 或直接推送 typed event（遵循现有 TEA effect 边界，优先 Effect）；删除 `run_loop.rs` 中接收 `Some(prompt)` 后再包装 `UserMessage` 的旁路。TUI 可显示短 system notice `[skill requested: release]`，但正文绝不进入 notice。

- [ ] **Step 6: no-TUI 共用 payload builder**

把 route → `ChatInputEvent::SkillRequest` 的纯转换放在 SDK/CLI adapter 的单一函数，TUI/no-TUI 共用；禁止复制参数 join、alias 还原和 InputId 生成逻辑。

- [ ] **Step 7: 运行 CLI 测试并提交**

Run:

```bash
cargo test -p cli command_contract -- --nocapture
cargo test -p cli tui::app::slash -- --nocapture
cargo test -p cli no_tui -- --nocapture
```

Expected: PASS；CLI 源码中不存在 `skill.content` 或 `Arguments: {args}` 拼接。

Commit:

```bash
git add agent/features/runtime/src/application/client agent/features/runtime/src/adapters/tui_launch.rs packages/sdk apps/cli
git commit -m "refactor: send skill requests from cli"
```

## Task 8：Skill Tool 展示隐藏正文并完成跨层场景

**Files:**
- Create: `apps/cli/src/tui/render/output/tool_display/tool_impls/skill.rs`
- Create: `apps/cli/src/tui/render/output/tool_display/tool_impls/skill_tests.rs`
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- Modify: `apps/cli/src/tui/render/output/tool_display/tests.rs`
- Modify: `apps/cli/src/tui/app/scenario_tests.rs`
- Create: `apps/cli/src/tui/app/scenario_tests/skill.rs`
- Modify: `agent/features/tools/tests/command_contract.rs`
- Modify: `agent/features/context/tests/skill_prompt_pipeline.rs`

- [ ] **Step 1: 写失败的展示测试**

输入 `{"skill":"release"}` 时 header 必须为 `Skill release`，policy 必须是 `details: Hidden`、`result: Hidden`。传入包含完整正文的 ToolResultPayload 后 framebuffer/渲染行中不得出现正文 sentinel。

- [ ] **Step 2: 实现 ToolDisplayEntry**

`SkillDisplay` 只从 input 读取 `skill`，不从 result data 读取 name，不回退输出正文；注册 canonical name `Skill`。未知/损坏 input 显示 `Skill ?`，不得泄漏 raw payload。

- [ ] **Step 3: 写 L4 场景测试**

用 TUI Harness 和 scripted provider/tool execution 完成：

1. Runtime/TUI 初始 metadata 有 `release`，但输出和 system block 无正文 sentinel；
2. 用户提交 `/release v1.2.3`，effect 是 SkillRequest；
3. provider 第一次响应调用 `Skill { skill: "release" }`，没有 args；
4. Tool 调用时读取临时 `SKILL.md`，对应 ToolResult 文本含正文 sentinel；
5. provider 第二次请求能看到正文与参考参数并返回最终回答；
6. TUI framebuffer 只看到 Skill header 与最终回答，看不到正文 sentinel。

再加删除竞态场景：步骤 1 后删除文件，步骤 3 的 ToolResult 是 typed failure，provider 可向用户说明且 Run 正常终结。

- [ ] **Step 4: 运行 L1-L4 定向测试并提交**

Run:

```bash
cargo test -p cli tool_display::tool_impls::skill -- --nocapture
cargo test -p cli scenario_tests::skill -- --nocapture
cargo test -p tools skill_tool_contract -- --nocapture
cargo test -p context skill_prompt_pipeline -- --nocapture
```

Expected: PASS；正文只对第二次模型调用可见，不对 TUI 可见。

Commit:

```bash
git add apps/cli agent/features/tools/tests agent/features/context/tests
git commit -m "feat: render dynamic skill calls safely"
```

## Task 9：退役旧路径、更新 Guard 与全量验证

**Files:**
- Modify: `.agents/hooks/check-tool-catalog-execution-boundary.sh`
- Modify: `.agents/hooks/check-tool-catalog-execution-boundary-tests.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `specs/runtime.md`
- Modify: `specs/tools.md`
- Modify: `specs/tui-cli.md`
- Modify: `specs/prompt.md`
- Delete/rename: all obsolete Skill materialization/PromptInjection tests identified below

- [ ] **Step 1: 先写 Guard 负例**

Sanity script 在隔离副本逐一注入并断言单 Guard exit 2：

1. `packages/sdk/src/tui.rs` 恢复 `SkillView.content`；
2. `apps/cli/src/tui/app/slash.rs` 恢复 `skill.content`；
3. Context skills block读取 `LoadedSkill.content` 或 `PromptFragment`；
4. Skill Tool schema 新增 `args`；
5. Tools 恢复第二个 legacy Skill Tool/SkillInput/SkillResult DTO；
6. Runtime 直接引用 `FilesystemSkillAdapter` 或读取 Skill 文件。

- [ ] **Step 2: 修改边界 Guard**

删除名称级 `SkillTool` 黑名单，因为新生产类型必须存在；改为禁止 `LegacyNoAgent`、legacy `SkillInput/SkillResult`、旧 factory 名和上述旁路模式。正向检查要求：Skill 只在 `registry.rs` 单一规格中注册；Runtime 仍只见 Catalog/Execution/Skill PL ports；Composition 是 filesystem adapter 唯一生产构造点。不得新增 allowlist/exclusion/suppression。

- [ ] **Step 3: 删除所有旧符号和兼容入口**

Run:

```bash
rg -n 'PromptFragment|SkillMaterialization|materialize_available|with_skill_supplier|skill\.content|SkillView.*content|CommandMechanism::PromptInjection|CommandRoute::PromptInjection|skill_not_tool' \
  agent packages apps specs docs/design
```

Expected: 生产代码零命中；测试/历史 changelog 只保留迁移说明，不存在可编译兼容 alias。同步检查无仅测试引用的死代码。

- [ ] **Step 4: 更新 specs 与 Guard 文档**

明确：Main/Sub Skill Tool scope、SkillRequest 事件、Run Step metadata 快照、调用时 live load、TUI hidden result、Command 分流、typed deletion failure。更新 Guard registry 时保持 17 个 guard 编排结构和 active policy 一致。

- [ ] **Step 5: 运行格式化、编译、测试和架构门禁**

Run:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p tools
cargo test -p context
cargo test -p sdk
cargo test -p runtime
cargo test -p composition
cargo test -p cli
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-tool-catalog-execution-boundary-tests.sh
bash .agents/hooks/check-architecture-guards.sh --full
bash .agents/hooks/check-production-reachability.sh
```

Expected: 全部 exit 0；若首次失败，只能修正根因，不能以重跑成功覆盖首次失败记录。

- [ ] **Step 6: 回填 Issue 门禁与最终提交**

在 #1438 comment 回填：开发前差异逐项“已对齐/已修正文档”；L1-L4 命令与结果；L5 N/A 理由；旧路径 `rg` 零命中；Guard 正反例。更新 milestone Release Gate checklist，但不关闭 Issue。

Commit:

```bash
git add .agents specs docs/design agent packages apps
git commit -m "chore: retire eager skill delivery"
```

## 最终验收矩阵

| 边界 | 必须证明的事实 | 证据层级 |
|---|---|---|
| Skill parser → Catalog | metadata 含 name/description/argument hint，不含正文 | L1/L2 |
| Catalog → Context | Run Step 目录稳定、受预算约束、无正文 | L2/L3 |
| Slash Router → SDK | Skill 与 Command 分类不同，identity/arguments 不丢失 | L1/L3 |
| SDK → Runtime gate | idle/busy/sealed/withdraw 保留 InputId 与用户原文 | L2/L3 |
| Runtime → LLM | SkillRequest 成为模型可见意图，参数只供参考 | L2/L4 |
| LLM → Skill Tool | schema 只有 skill；Main/Sub 正式 Catalog 可见 | L3 |
| Skill Tool → filesystem | 调用时 live root + frozen dirs/tools，删除返回 typed failure | L2/L3 |
| ToolResult → LLM/TUI | LLM 见正文；TUI 只见 header，result hidden | L2/L4 |
| Command handler | `/clear` 等继续确定性执行，不进入 SkillRequest | L3/L4 |
| 架构收口 | 无 PromptFragment 全文注入、无 TUI content 副本、无 legacy alias | L0 |

## 非目标

- 不为每个 Skill 生成动态 Tool schema。
- 不在本 Issue 支持 `$ARGUMENTS` 模板替换；参数已作为 LLM 参考上下文交付。
- 不让 TUI 或 Runtime 直接执行 Skill Tool。
- 不把 Skill 调用改成 Command handler。
- 不新增 Skill 正文缓存、watch/hot reload 或 MCP remote Skill 协议。
- 不在本 Issue 引入 `allowed-tools` 动态扩权、model override、forked Skill 或 hook 注册；这些需要独立安全设计和用户确认。
- 不增加 L5 真终端测试；进程内 L4 已覆盖完整 typed 链路。

