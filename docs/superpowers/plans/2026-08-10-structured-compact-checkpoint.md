# Auto-compact 结构化 Checkpoint 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 auto-compact 的 map、reduce、refresh 全部改成 typed JSON 管线，并由 Context 单一 renderer 确定性生成兼容九段 Markdown checkpoint。

**Architecture:** Context domain 新增局部事实与 checkpoint wire 类型，adapter 只负责提示、LLM 调用和 JSON 解码；领域归并器按来源顺序、scope 和 lifecycle 解析授权，refresh invariant 拒绝关键语义丢失或升级。既有 `ContinuationCheckpoint` 成为最终领域对象和唯一 Markdown renderer，legacy Markdown 仅保留为 previous summary 兼容读取入口。

**Tech Stack:** Rust、Serde/serde_json、Tokio、现有 `CompactGenerator` Port、cargo test/clippy/fmt、架构守卫。

---

## 文件结构

- 新建 `agent/features/context/src/domain/compact/structured_facts.rs`：定义 map 局部事实、来源、scope、lifecycle、约束动作及确定性归并规则。
- 新建 `agent/features/context/src/domain/compact/structured_facts_tests.rs`：覆盖 JSON 严格解析、scope 降级、授权顺序与跨 scope 不升级。
- 修改 `agent/features/context/src/domain/compact/continuation_checkpoint.rs`：增加 typed checkpoint wire 转换、protected refresh fingerprint/invariant，保留唯一 renderer 与 legacy parser。
- 修改 `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`：覆盖 typed wire、refresh 保护和 renderer 兼容。
- 修改 `agent/features/context/src/domain/compact.rs`：注册并窄 re-export structured compact 类型。
- 修改 `agent/features/context/src/adapters/compact_summary.rs`：将 map/reduce/refresh 改为 JSON 请求和 typed 解码；previous/fallback 汇入领域类型；删除自由文本阶段协议。
- 修改 `agent/features/context/src/adapters/compact_summary_tests.rs`：使用 scripted generator 驱动全阶段 JSON，覆盖错误分类、fallback 和端到端模板输出。
- 修改 `agent/features/context/tests/canonical_session_repository.rs`：验证 typed LLM checkpoint 经 durable compact 后仍以九段 `active_summary` 恢复。
- 修改 `docs/design/02-modules/context-management/02-compact.md`：同步 typed pipeline、scope/lifecycle 与 renderer Target 契约。

### Task 1：建立 typed map facts 契约

**Files:**
- Create: `agent/features/context/src/domain/compact/structured_facts.rs`
- Create: `agent/features/context/src/domain/compact/structured_facts_tests.rs`
- Modify: `agent/features/context/src/domain/compact.rs`

- [ ] **Step 1: 先写 JSON round-trip 与 unknown-field 失败测试**

定义测试 fixture，覆盖 `CompactFactBatch { facts }`，其中每个事实包含 `sequence`、`source`、`kind`、`text`、可选 `constraint`。反序列化类型使用 `#[serde(deny_unknown_fields)]`，测试未知字段返回错误。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p context structured_facts -- --nocapture`

Expected: FAIL，原因是 `structured_facts` 模块或类型尚不存在。

- [ ] **Step 3: 实现领域类型**

实现以下受限枚举并统一 `snake_case` 序列化：

- `CompactFactSource::{MainUser, AssistantReport, ToolInvocation, ToolResult, SystemGenerated, SubagentInstruction, Unknown}`
- `ConstraintScope::{Session, Task, Phase, ToolCall, Unknown}`
- `ConstraintLifecycle::{Persistent, UntilTaskEnd, UntilPhaseEnd, UntilToolCallEnd, Unknown}`
- `ConstraintAction::{Grant, Restrict, Revoke, Supersede}`
- `CompactFactKind::{Constraint, Objective, CommittedFact, WorkingSet, Risk, ResumeCandidate, Revalidation, Milestone}`

构造函数必须验证：空文本非法；`kind=constraint` 必须带 constraint metadata；其他 kind 禁止携带 constraint metadata；sequence 用 `u64`。

- [ ] **Step 4: 实现 scope 安全归一化**

规则固定为：只有 `source=main_user` 可保留 `scope=session`；其他来源声明的 session scope 必须降为 `unknown`，并转入 risk，禁止进入 immutable constraints。`SubagentInstruction`、`SystemGenerated` 和 `Unknown` 不能产生主 Session grant/restrict/revoke/supersede。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p context structured_facts -- --nocapture`

Expected: PASS。

- [ ] **Step 6: 提交领域事实契约**

Run:

```bash
git add agent/features/context/src/domain/compact.rs \
  agent/features/context/src/domain/compact/structured_facts.rs \
  agent/features/context/src/domain/compact/structured_facts_tests.rs
git commit -m "feat(context): add typed compact fact contract"
```

### Task 2：将 typed facts 归并为 continuation checkpoint

**Files:**
- Modify: `agent/features/context/src/domain/compact/structured_facts.rs`
- Modify: `agent/features/context/src/domain/compact/structured_facts_tests.rs`
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`

- [ ] **Step 1: 先写授权生命周期回归测试**

覆盖以下顺序：tool-call 只读限制后出现 main-user 实施授权，最终 immutable constraints 不得包含 tool-call 只读；同一 session scope 的 later revoke/supersede 必须替代早期 restrict；unknown scope 不得扩大成 session scope。

- [ ] **Step 2: 写 checkpoint wire 与 renderer 测试**

定义 `ContinuationCheckpointWire` 的 JSON fixture，字段对应九段语义但不含 Markdown 标题；验证 `try_from_wire(...).render()` 仍产生固定顺序九段标题且唯一 `Next action`。

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p context continuation_checkpoint structured_facts -- --nocapture`

Expected: FAIL，缺少 wire 转换和归并器。

- [ ] **Step 4: 实现顺序归并器**

按 `(sequence, 原始位置)` 稳定排序。约束只在同一 scope identity 内执行 grant/restrict/revoke/supersede；task/phase/tool_call 缺少可证明 identity 时保持 scoped risk，不提升为 immutable constraint。目标取最新 main-user objective；committed fact 只接受 ToolResult 或 durable-evidence 标记；assistant 报告进入 working set/risk。

- [ ] **Step 5: 实现 typed checkpoint wire 转换**

`ContinuationCheckpointWire` 使用 `deny_unknown_fields`；`next_action` 为单一字符串而非数组；`status` 复用可 serde 的 `ContinuationStatus`。转换时调用现有 `ContinuationCheckpoint::from_sections`，Markdown renderer 仍只保留一处。

- [ ] **Step 6: 运行定向测试确认通过**

Run: `cargo test -p context continuation_checkpoint structured_facts -- --nocapture`

Expected: PASS。

- [ ] **Step 7: 提交归并与 wire 类型**

Run:

```bash
git add agent/features/context/src/domain/compact/structured_facts.rs \
  agent/features/context/src/domain/compact/structured_facts_tests.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs
git commit -m "feat(context): reduce compact facts into typed checkpoint"
```

### Task 3：建立 refresh 受保护语义 invariant

**Files:**
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs`

- [ ] **Step 1: 先写 refresh 拒绝测试**

分别验证 refresh 不能删除/改写 immutable constraints、不能提升 current objective 动作级别、不能改变 next action、不能删除 prohibited lines、不能把 `WaitingForUser` 改为 `Continue`、不能删除 waiting reason。

- [ ] **Step 2: 写允许压缩测试**

验证 refresh 可以减少 committed facts、working set、risks 和 archived milestones，只要受保护字段逐字保持且 token 更少。

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p context refresh -- --nocapture`

Expected: FAIL，缺少 refresh invariant。

- [ ] **Step 4: 实现 `validate_refresh_from`**

以 typed 字段比较而非 Markdown 文本搜索：immutable constraints、current objective、resume next action、resume prohibited lines、status 和 status reason 构成 protected identity；新 checkpoint 必须等于旧值。普通字段只允许减少或重述，不参与权限判断。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p context refresh -- --nocapture`

Expected: PASS。

- [ ] **Step 6: 提交 refresh invariant**

Run:

```bash
git add agent/features/context/src/domain/compact/continuation_checkpoint.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint_tests.rs
git commit -m "fix(context): protect compact semantics during refresh"
```

### Task 4：将单块 map 调用改为 typed JSON

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [ ] **Step 1: 先改测试 generator 返回 `CompactFactBatch` JSON**

新增 `ScriptedCompactGenerator`，以线程安全队列按调用顺序返回 JSON，并记录请求文本。单块测试断言提示词要求“JSON only”、包含 schema 字段说明、不包含九个 Markdown 标题或 `<summary>` 标签。

- [ ] **Step 2: 运行单块测试确认失败**

Run: `cargo test -p context compact_with_generator_uses_llm_summary -- --nocapture`

Expected: FAIL，当前实现仍解析 `<summary>` Markdown。

- [ ] **Step 3: 实现通用 typed JSON 解码**

替换 `parse_compact_response`/`llm_generate` 为泛型或阶段专用 decode helper：去除可选 fenced JSON 外壳后用 `serde_json::from_str` 解码；空文本、非法 JSON、未知字段统一映射到 `InvalidSummary`，错误消息带 `map`/`reduce`/`refresh` stage。

- [ ] **Step 4: 构建 map 请求**

`build_compact_request` 输出局部事实 schema 指令。序号由 Context 给每条输入消息分配稳定区间，LLM 只能引用给定 sequence；Message metadata 映射为 `MainUser/SystemGenerated`，assistant/tool block 分别映射来源。不能从当前 Message 数据证明 subagent identity 时，提示模型使用 `unknown`，不得猜 session scope。

- [ ] **Step 5: 单块路径本地归并并渲染**

单块返回 `CompactFactBatch` 后直接调用领域归并器生成 `ContinuationCheckpoint`，归一化预算，再由 `render()` 生成 summary。

- [ ] **Step 6: 运行相关测试确认通过**

Run: `cargo test -p context compact_summary_tests -- --nocapture`

Expected: 单块、fallback、取消、进度测试通过；map-reduce 测试可暂时仍失败并在下一任务修复。

- [ ] **Step 7: 提交单块 typed map**

Run:

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs
git commit -m "feat(context): decode typed compact map facts"
```

### Task 5：将 map-reduce 改为 typed facts 和 typed checkpoint

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [ ] **Step 1: 先写多块 scripted 测试**

按 N 个 map 响应加一个 reduce 响应排队。断言 map 响应解码为 facts；reduce 请求包含 JSON facts 数组；reduce 返回 `ContinuationCheckpointWire`；并发完成顺序不同仍按 chunk index/sequence 稳定合并。

- [ ] **Step 2: 写跨 chunk 权限回归测试**

chunk 1 包含 tool-call scoped `restrict`，chunk 2 包含 later main-user session `grant/supersede`；最终 Markdown 不得出现全 Session 禁止写入，当前目标和 next action 必须使用 later user 要求。

- [ ] **Step 3: 运行 map-reduce 测试确认失败**

Run: `cargo test -p context map_reduce -- --nocapture`

Expected: FAIL，reduce 仍消费 Markdown 分段摘要。

- [ ] **Step 4: 实现 typed map-reduce**

并发 map 返回 `(chunk_index, CompactFactBatch)`；按 chunk index 排序并重编号/验证 sequence；reduce prompt 只包含 JSON facts 与 checkpoint wire schema；decode 后执行本地 scope validator、checkpoint normalizer 和 renderer。

- [ ] **Step 5: 运行 map-reduce 测试确认通过**

Run: `cargo test -p context map_reduce -- --nocapture`

Expected: PASS，包括并发上限、进度和 chunk 数测试。

- [ ] **Step 6: 提交 typed reduce**

Run:

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs
git commit -m "feat(context): reduce typed compact facts"
```

### Task 6：将 refresh 改为 typed checkpoint

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`

- [ ] **Step 1: 先写 typed refresh scripted 测试**

让 reduce 返回超预算 wire，refresh 返回更短 wire；断言请求传递 JSON checkpoint，不传 Markdown，并在受保护字段不变时接受。

- [ ] **Step 2: 写恶意 refresh 回归测试**

refresh 删除只读/授权边界或把 `WaitingForUser` 改为 `Continue` 时，结果必须拒绝并保留上一版较长 checkpoint；不得转成新的保守 session 限制。

- [ ] **Step 3: 运行 refresh 测试确认失败**

Run: `cargo test -p context refresh -- --nocapture`

Expected: FAIL，refresh 仍使用 Markdown。

- [ ] **Step 4: 实现 typed refresh**

`llm_refresh` 接收 `&ContinuationCheckpoint`，序列化 wire 后请求更短 wire；decode 后先 `validate_refresh_from`，再比较 token 数。非法 refresh 记阶段化 warning，并沿用原 checkpoint；取消继续中止且不 fallback。

- [ ] **Step 5: 运行 refresh 测试确认通过**

Run: `cargo test -p context refresh -- --nocapture`

Expected: PASS。

- [ ] **Step 6: 提交 typed refresh**

Run:

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs
git commit -m "fix(context): validate typed compact refresh"
```

### Task 7：统一 previous summary 与 local fallback

**Files:**
- Modify: `agent/features/context/src/adapters/compact_summary.rs`
- Modify: `agent/features/context/src/adapters/compact_summary_tests.rs`
- Modify: `agent/features/context/src/domain/compact/continuation_checkpoint.rs`

- [ ] **Step 1: 先写 previous checkpoint typed 输入测试**

验证现有九段 Markdown 只在边界解析一次为 `ContinuationCheckpoint`，随后以 JSON wire 进入 map/reduce；`Current Task State` companion 继续剥离且不发送给 LLM。

- [ ] **Step 2: 先写 fallback 不产生虚构 session 权限测试**

历史只包含 subagent/tool-call “只读”时，fallback 的 immutable constraints 不得生成主 Session 禁写；无法确认 scope 的限制进入 risk/revalidation。最新主用户明确批准实施时，current objective/next action 反映实施，但不自行增加 commit/push/merge 权限。

- [ ] **Step 3: 运行 fallback 测试确认失败**

Run: `cargo test -p context fallback -- --nocapture`

Expected: 至少权限 scope 回归 FAIL。

- [ ] **Step 4: 重写 local fallback 为 typed 构造**

本地扫描直接生成 `CompactFactBatch`，复用同一领域归并器；删除硬编码的全 Session “Preserve action level; do not infer new authority” immutable constraint，改为 scope 不明风险与 required revalidation。仅最终调用 renderer。

- [ ] **Step 5: 清理自由文本阶段协议**

删除 `<summary>` parser、Markdown reduce part 拼装、LLM Markdown normalizer 和不再使用的测试常量。保留 `ContinuationCheckpoint::parse` 仅用于持久化 legacy/current nine-section 兼容读取。

- [ ] **Step 6: 运行 compact 全部测试**

Run: `cargo test -p context compact -- --nocapture`

Expected: PASS。

- [ ] **Step 7: 提交统一 fallback**

Run:

```bash
git add agent/features/context/src/adapters/compact_summary.rs \
  agent/features/context/src/adapters/compact_summary_tests.rs \
  agent/features/context/src/domain/compact/continuation_checkpoint.rs
git commit -m "fix(context): unify compact fallback through typed facts"
```

### Task 8：补 durable session 契约和设计文档

**Files:**
- Modify: `agent/features/context/tests/canonical_session_repository.rs`
- Modify: `docs/design/02-modules/context-management/02-compact.md`

- [ ] **Step 1: 先写 durable compact 集成测试**

更新 fixed generator 返回 typed JSON。断言 compact commit 后 `active_summary` 是九段 renderer 输出，resume 后逐字一致，quality 为 LLM；非法 JSON fallback 保留对应 failure kind。

- [ ] **Step 2: 运行集成测试确认失败**

Run: `cargo test -p context --test canonical_session_repository commit_compaction_with_generator_uses_llm_summary -- --nocapture`

Expected: FAIL，fixture 仍使用 Markdown 响应。

- [ ] **Step 3: 更新集成 fixture 并跑通**

复用测试模块内 JSON helper，禁止复制生产归并逻辑；断言最终 summary 可被 `ContinuationCheckpoint::parse` 读取。

- [ ] **Step 4: 核对并完成设计文档门禁**

确认文档明确 map facts、reduce/refresh typed checkpoint、scope/lifecycle、single renderer、legacy Markdown 边界与错误语义；正文不引用 Issue/PR 编号，只在修改历史保留关联。

- [ ] **Step 5: 运行集成测试**

Run: `cargo test -p context --test canonical_session_repository -- --nocapture`

Expected: PASS。

- [ ] **Step 6: 提交集成契约与文档**

Run:

```bash
git add agent/features/context/tests/canonical_session_repository.rs \
  docs/design/02-modules/context-management/02-compact.md
git commit -m "test(context): cover durable typed compact checkpoint"
```

### Task 9：完整验证、Issue 门禁与 PR

**Files:**
- Modify only if validation reveals an in-scope defect.

- [ ] **Step 1: 格式与差异检查**

Run: `cargo fmt --all -- --check && git diff --check`

Expected: PASS。

- [ ] **Step 2: context 全测试**

Run: `cargo test -p context`

Expected: PASS，0 failed。

- [ ] **Step 3: context clippy**

Run: `cargo clippy -p context --all-targets -- -D warnings`

Expected: PASS，0 warnings。

- [ ] **Step 4: 架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`

Expected: PASS；如脚本要求其他入口，按输出运行仓库登记的等价 guard，禁止绕过。

- [ ] **Step 5: 检查死代码和废弃协议残留**

Run: `rg -n '<summary>|Part [0-9]+ summary|Write your summary inside|normalize_generated_checkpoint' agent/features/context/src`

Expected: 不存在 map/reduce/refresh 自由文本阶段协议；仅允许明确的 legacy compatibility 引用，并在 PR 说明理由。

- [ ] **Step 6: 更新 Issue 清单和 Release Gate**

用 `gh issue view 1582 --repo rushsinging/aemeath` 逐项核对 checklist；已完成项勾选，N/A 项记录证据。确认 #579 仍关联 #1582。

- [ ] **Step 7: 同步最新 main**

Run: `git pull origin main`

Expected: 成功合并或快进；若冲突，逐项保留双方测试保护行为并重跑 Step 1–4。

- [ ] **Step 8: 推送并创建 PR**

PR 标题：`fix(context): structure auto-compact checkpoint pipeline`

PR body 必须包含：根因、typed map/reduce/refresh、scope 防升级、legacy 兼容、文档核对、完整 Test plan，以及 `Closes #1582`。

- [ ] **Step 9: 查询 PR 状态**

Run: `gh pr view <PR编号> --repo rushsinging/aemeath --json url,state,isDraft,mergeable,mergeStateStatus,statusCheckRollup,headRefOid`

Expected: PR OPEN、非 Draft；报告真实 required checks 状态，未经当前 head 的具体授权不合并。
