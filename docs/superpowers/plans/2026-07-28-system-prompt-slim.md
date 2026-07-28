# v0.1.0 System Prompt 精简实施计划

> 对应 Issue：[#1448](https://github.com/rushsinging/aemeath/issues/1448)
> 分支：`feat/1448-system-prompt-slim`
> 基线：`origin/main` @ `7940df0f`
> Milestone：`v0.1.0 — Context Engineering + 架构重构`

## 1. 目标与完成定义

精简 Aemeath 内置 System Prompt，使现代 LLM 获得更短、更稳定、无重复的行为契约，同时不削弱以下能力：

- 工具调用的真实性与专用工具优先原则；
- 文件修改、外部副作用和权限模式的安全边界；
- 修改后的验证要求；
- Main/Sub 隔离及 Agent prompt 自包含语义；
- `path_base`、`workspace_root` 和 worktree 上下文切换；
- Skills、Memory、user guidance、task reminder 的现有顺序、预算、缓存和安全扫描语义。

完成条件：

1. 每个稳定 System Block 只有一个生产 owner；主会话不再重复注入 `execution_discipline`。
2. 核心静态 Prompt 只保留 Aemeath 特有、不能由 Tool Schema 或 Runtime 强制保证的契约。
3. 工具级细节由工具 description/schema 承载；模型差异由 model guidance 承载；不在核心 Prompt 重复描述。
4. Main 与 Sub 的 system block 组合遵循同一 Context 物化契约，role suffix 仅作为显式扩展。
5. Prompt 组合顺序、cacheable prefix、唯一 cache breakpoint 和已有功能行为保持兼容。
6. 有可追溯的 L0、L1/L2、L3、L4 验证证据，并记录精简前后 token/byte 基线。

## 2. 当前根因与约束

### 2.1 已确认的重复注入

当前 Main 启动流程在 `agent/features/runtime/src/application/prompt/prompt_build_ext.rs` 中将：

```text
static_part + execution_discipline + agent_roles + model_guidance
```

拼成 `system_prompt_text`；随后 Context 的 `BaselinePromptSource` 又从 `ContextRequest` 生成：

```text
system_prompt + execution_discipline
```

因此 Runtime 传入的 `system_prompt` 已可能包含执行纪律，Context 又追加独立的 `execution_discipline` block，导致重复。

### 2.2 已确认的下沉机会

- 文件工具 description 已包含 `Read first`、精确替换和 `old_string`/`new_string` 语义。
- Worktree 工具已包含 `path_base` / `workspace_root` 和上下文切换后的路径规则。
- Agent 工具已包含隔离会话、自包含 prompt、角色模型和子代理限制说明。
- Task 工具已包含复杂任务阈值、单一可验证步骤和生命周期操作说明。
- Tools BC 已将 `phase` 作为运行时元数据剥离，不属于业务 schema。

因此核心 System Prompt 不应继续逐项复制这些工具级说明。

### 2.3 设计基准

沿用 `docs/design/02-modules/context-management/04-prompt-guidance.md` 的约束：Context Window assembler 负责最终 System Block 编排，稳定前缀顺序为：

```text
system_prompt
→ execution_discipline
→ model_guidance
→ skills
→ agent_roles
→ user_guidance
→ memory_context
→ active_summary
→ 唯一 cache breakpoint
→ task_reminder
```

Git 首次快照继续作为普通系统生成消息；日期、工作区变化和 commit guidance 不进入 System Prompt。

## 3. 不在本期范围

- 不改变 Tool 的业务 schema、权限策略、workspace ACL 或执行逻辑。
- 不重写 Skills 的 source discovery、解析、预算或安全扫描算法。
- 不改变 Memory 检索、Reflection、compact 和 Task 状态机。
- 不新增模型能力探测或 A/B Prompt profile；本期先建立精简后的单一现代默认 Prompt，模型专属差异继续使用既有 guidance。
- 不为了迁移而批量移动无关测试或文档。
- 不修改用户自定义 `~/.agents/guidance/` 内容及其优先级语义。

## 4. 分阶段实施

### 阶段 A：建立 Prompt 资产清单与基线

**目标**：在任何行为改动前锁定现状和每条规则的归属。

**步骤**：

1. 列出所有面向 LLM 的内置文案来源：
   - `agent/shared/src/i18n/prompt/system.rs`
   - `agent/shared/src/i18n/prompt/discipline.rs`
   - `agent/shared/src/i18n/prompt/sections.rs`
   - `agent/shared/src/i18n/prompt/commit.rs`
   - `agent/shared/src/i18n/tools/**`
   - `agent/features/runtime/src/application/prompt/**`
   - `agent/features/context/src/adapters/prompt/**`
   - 子代理启动文案。
2. 为每个片段记录：当前 owner、注入路径、语言版本、cacheable 状态、是否重复、是否已有 Tool/Runtime 强制、目标去向。
3. 增加或复用测试辅助函数，构造代表性 Main Prompt 和 Context Window，输出 UTF-8 byte 数与 token estimate；保存精简前基线，不把完整 Prompt 内容写入生产日志。
4. 明确静态 Prompt 的保留清单与删除清单，作为后续测试断言的依据。

**交付物**：Prompt 资产矩阵、精简前 baseline、保留/下沉决策表。

**验证**：L1 纯函数/组装测试；L0 确认基线仅使用测试入口，不扩大生产 API。

### 阶段 B：收口唯一基础 Prompt owner

**目标**：消除主会话重复执行纪律，并让 `ContextPromptSource` 成为最终基础 System Block 的唯一 owner。

**步骤**：

1. 将 `static_system_prompt_for()` 的职责限定为核心 `system_prompt`：身份、工具真实性、必要安全边界、验证原则、环境/worktree 语义和子代理隔离摘要。
2. 从 Runtime 的 `build_static_prompt()` 移除 `universal_execution_discipline` 的拼接；Runtime 不再把执行纪律嵌入 `SystemPromptSpec`。
3. 保留 `BaselinePromptSource` 对 `system_prompt` 与 `execution_discipline` 的单次 materialization。
4. 确认 `SkillPromptSource` 复用同一 baseline，不产生第二套基础 block。
5. 若当前 Main 启动仍需传递 model guidance、roles 或 user guidance，改为通过 Context-owned materialization 的明确输入传递；禁止继续在 Runtime 先拼完整字符串再交给 Context。
6. 统一 Main/Sub 的 system block 入口；role-specific suffix 只在 Sub Run 的显式 role 扩展点追加一次。

**交付物**：无重复的基础 Prompt 管线；Main/Sub 对应的 owner 和输入边界。

**验证**：

- L1：`system_prompt` 不包含 `execution_discipline`，语言和权限模式替换仍正确。
- L2：baseline materialization 恰好产生预期 kind 和顺序。
- L3：Main/Sub invocation 均能完整映射 Context blocks；Provider 未收到重复基础块。

### 阶段 C：精简核心静态 Prompt

**目标**：把百余行流程规约压缩为短的产品契约。

**保留**：

- Agent 身份和“以完成且验证用户目标为准”；
- 涉及文件、命令或计算时必须使用真实工具，不得虚构结果；
- 有专用工具时优先使用专用工具；
- 修改或有副作用操作遵循当前权限/确认机制；
- 修改后按范围执行验证，未验证不得声称完成；
- 子代理是隔离会话，prompt 必须自包含；
- 当前 `path_base`、`workspace_root`、git/worktree 上下文必须以最新工具结果为准；
- 基本安全要求：不得引入常见注入、越权、凭据泄露风险；
- 用户未要求时保持简洁。

**下沉/删除**：

- Read/Edit/Write/Glob/Grep 的逐项替代表格：由工具 description 承载；
- Agent 两阶段浏览细则和 BAD/GOOD 示例：由 Agent 工具/相关 skill 承载；
- TaskList 全生命周期、依赖和示例：由 Task 工具与 Runtime 状态维护承载；
- `phase` 完整教程：保留 Tools 元数据契约，核心 Prompt 只保留必要的一句或移除；
- 12,000 字符、SSE、provider 输出预算等实现细节：移至 Write/Prompt 工具 description 或开发文档；
- Bug 最小补丁/根因方案、TDD、架构审查等长流程：由对应 skill 和仓库 guidance 承载；
- 模型 reasoning 语言/长度要求：保留在模型 guidance，不放入通用核心 Prompt；
- 重复的 `old_string`/`new_string`、AskUserQuestion 选项细则和 commit workflow。

**注意**：只删除重复指导，不删除工具本身的 description/schema，也不删除 Runtime 的安全校验。

**验证**：

- L1：EN/ZH 选择、占位符、权限模式和关键保留契约断言；
- L2：精简后 baseline 文本不存在已下沉长段落；
- L4：文件探索、编辑、验证、worktree 切换、Agent 派发代表性场景不退化。

### 阶段 D：清理动态组装与过期路径

**目标**：删除迁移遗留，避免后续再次把规则拼回核心 Prompt。

**步骤**：

1. 检查 `prompt_build_ext.rs` 中 model guidance、agent roles、skills header 的实际 owner；将仍由 Runtime 拼接的稳定 System Block 迁入 Context Prompt materialization，或明确保留 Runtime 输入为独立 typed fragment，不允许裸字符串混合。
2. 清理不再使用的拼接 helper、旧常量、re-export、测试专用生产入口和过期注释。
3. 检查 `SystemPromptSpec`、`PromptMaterialization`、`SystemBlock` 的字段是否仍能表达唯一 owner、kind、revision、cacheable/cache_break；不为兼容而保留第二套拼装模型。
4. 检查 Main/Sub role suffix 与 Hook 输入，确保 Hook 看到的 system 文本与实际 provider invocation 一致。
5. 同步更新 `specs/prompt.md`、`specs/runtime.md` 和 Context Prompt 设计文档中的顺序、owner 与“日期/commit guidance 不注入”描述。

**验证**：L0 架构守卫、L2 组装顺序、L3 Runtime/Context/Provider 契约和死代码检查。

### 阶段 E：行为回归与基线对比

**目标**：证明精简减少了 Prompt，但未牺牲关键行为。

**步骤**：

1. 对同一模型、语言、权限模式和工具集合分别生成 before/after baseline。
2. 比较：总 bytes、估算 tokens、cacheable prefix bytes、各 block kind、重复文本 hash。
3. 验证 Main/Sub 发送的 system block 序列完全符合契约，且不出现重复 `execution_discipline`。
4. 执行代表性场景：
   - 只读探索；
   - 精确 Edit/Write；
   - 修改后运行验证；
   - allow/ask 权限模式；
   - EnterWorktree/ExitWorktree 后路径切换；
   - Main 派发 Sub Agent；
   - Skill、Memory、active summary、task reminder 同时存在；
   - guidance / skill injection warning 不被精简逻辑绕过。
5. 将所有未完成或不适用的 Issue checklist 项记录在 Issue 或 PR 中，说明理由、影响和后续处理。

**验证命令**：

```bash
cargo test -p shared
cargo test -p context
cargo test -p runtime
cargo test -p tools
cargo test -p provider
cargo test -p composition
cargo check
cargo clippy --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
```

实际 crate 名称以 workspace `Cargo.toml` 为准；命令执行前先核对 package 名称，失败时按失败层级定向修复，不用重跑掩盖首次失败。

## 5. 测试策略与门禁

### L0

- production-only build / check；
- architecture guards；
- clippy；
- 检查被删除的拼接函数和常量没有生产引用；
- 检查测试专属 API 未泄漏。

### L1

- 静态 Prompt 语言 fallback；
- cwd/git/permission mode 替换；
- Prompt 分类和重复 hash；
- 预算/byte/token 计算辅助函数；
- 保留关键契约的语义断言，而不是锁定完整长字符串快照。

### L2

- `BaselinePromptSource`、`SkillPromptSource` 与 Context assembler 的 block kind、顺序、缓存断点；
- guidance / roles / skills / memory / summary / reminder 组合；
- empty、disabled、supplier failure 和 revision 变化。

### L3

- Runtime → Context Port → Provider `RequestSystemBlock` 映射；
- Main/Sub 使用同一 block 契约；
- 工具 metadata `phase` 剥离与业务 schema 兼容；
- Hook 看到的 system 文本与实际 invocation 一致。

### L4

- 主会话和子代理真实用户旅程；
- 权限拒绝、worktree 切换、文件修改和验证；
- Skill 安全扫描、Memory 注入和 task reminder 同时存在时的最终上下文。

测试必须先建立失败/回归证据，再修改生产逻辑；不以弱化断言换取通过。新增测试按 owning layer 放置，避免跨层万能 fixture。

## 6. 风险与回滚

| 风险 | 控制措施 |
|---|---|
| 精简后模型不再遵循某个 Aemeath 特有协议 | 只删除已有 Tool/Runtime 强制或重复条款；L4 保留场景回归；必要时恢复单条契约，不恢复整段流程说明 |
| Runtime/Context 双重组装导致顺序变化 | 先锁定 kind/order/cache breakpoint 契约，再迁移 owner |
| Main/Sub 行为不一致 | 共享 Context materialization 与 L3 双端契约测试 |
| 用户 Guidance 与内置 Prompt 冲突 | 保留既有 revision、语言、来源和注入优先级；不改用户文件内容 |
| cache 前缀失效或成本回退 | 对 before/after 的 cacheable prefix 和唯一 breakpoint 做基线比较 |
| 删除旧路径后出现死代码/隐式依赖 | 每阶段运行 production check、架构守卫和定向 grep；完成后清理废弃路径 |

若 L4 发现关键行为退化，优先恢复缺失的最小产品契约；不得把完整执行手册重新塞回核心 Prompt。若连续三次单点修复仍无法稳定，暂停实现并重新评估 owner/Context 架构。

## 7. 交付顺序

1. Prompt 资产矩阵和 before baseline；
2. 基础 Prompt 唯一 owner 与重复注入修复；
3. 核心 Prompt 精简；
4. 动态组装 owner 收口和废弃路径清理；
5. L0–L4 验证、after baseline 和 Issue checklist 更新；
6. 在 worktree 分支上 `git pull origin main` 后创建 PR，等待用户 review；agent 不自行合并或关闭 Issue。
