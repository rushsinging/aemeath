# #1964 全工具 `goal` + BackgroundProcess summary/TUI 映射 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 全内置工具必填 `goal`；TUI Header=`{友好名} {goal}`、Details=技术载荷；后台 `summary="{友好名} · {goal}"`；Watch/Status/List 用户面可见；L4+L5 覆盖全部内置工具展示。

**Architecture:** Schema 统一 `goal` → ToolDisplay 消费 → `execution_supervisor::invocation_summary_text` 写入账本 → Background* Display 映射。定稿见 `docs/design/02-modules/tui/06-tool-goal-and-background-summary.md`。

**Tech Stack:** Rust / serde / inventory ToolDisplay / sdk re-export / ratatui scenario + PTY L5

**Issue:** https://github.com/rushsinging/aemeath/issues/1964  
**Worktree branch:** `feature-1964-tool-goal-bg-summary`

---

## 拍板摘要（勿改）

| 项 | 值 |
|---|---|
| goal | 全内置工具必填 |
| Agent | `description` → `goal` + serde alias |
| Header | `{友好名} {goal}` |
| Details | command/prompt/query/path/… |
| summary | `{友好名} · {goal}` |
| Watch header | `Backend Process Watch · {summary} · {短尾}` |
| 交付 | 单 PR |
| L4/L5 | 覆盖全部内置 ToolDisplay 工具 |

---

### Task 1: 设计文档已落盘核验

**Files:**
- Verify: `docs/design/02-modules/tui/06-tool-goal-and-background-summary.md`

- [ ] **Step 1: 打开设计文档并确认 D1–D9 与 Issue body 一致**

```bash
rg -n '^\| D[0-9]' docs/design/02-modules/tui/06-tool-goal-and-background-summary.md
gh issue view 1964 --repo rushsinging/aemeath --json body --jq .body | head -40
```

- [ ] **Step 2: 提交设计+本计划（若尚未提交）**

```bash
git add docs/design/02-modules/tui/06-tool-goal-and-background-summary.md \
  docs/superpowers/plans/2026-10-13-issue-1964-tool-goal-bg-summary.md
git commit -m "$(cat <<'EOF'
docs: #1964 工具 goal 与 Background summary 定稿设计与计划

Co-Authored-By: Aemeath (Zhipu Coding Plan/glm-5.3) <github:rushsinging/aemeath>
EOF
)"
```

---

### Task 2: 写失败测试 — `invocation_summary_text` 语义摘要

**Files:**
- Modify: `agent/features/runtime/src/application/tool/execution_supervisor_tests.rs`
- Modify: `agent/features/runtime/src/application/tool/execution_supervisor.rs`（仅在 RED 确认后）

- [ ] **Step 1: 新增失败断言（期望 `Run · 跑 ping`，当前仍是 `tool=Bash input=`）**

```rust
#[test]
fn invocation_summary_uses_display_name_and_goal() {
    let call = /* 构造 SupervisedToolCall: tool_name=Bash, input={"goal":"跑 ping","command":"ping -c 1 1.1.1.1"} */;
    let text = invocation_summary_text(&call); // 若 fn 非 pub(crate)，同文件测或测公开路径
    assert_eq!(text, "Run · 跑 ping");
    assert!(!text.contains("input="));
}
```

- [ ] **Step 2: 跑测确认 RED**

```bash
cargo test -p runtime invocation_summary_uses_display_name_and_goal -- --nocapture
```

Expected: FAIL（仍含 `tool=` / `input=` 或不等）

- [ ] **Step 3: 实现 `invocation_summary_text` 读 goal + 友好名；缺 goal fallback `(no goal)`**

- [ ] **Step 4: 跑测确认 GREEN**

```bash
cargo test -p runtime invocation_summary -- --nocapture
```

- [ ] **Step 5: commit**

```bash
git add agent/features/runtime/src/application/tool/execution_supervisor.rs \
  agent/features/runtime/src/application/tool/execution_supervisor_tests.rs
git commit -m "$(cat <<'EOF'
fix(runtime): #1964 invocation_summary 改为友好名·goal

Co-Authored-By: Aemeath (Zhipu Coding Plan/glm-5.3) <github:rushsinging/aemeath>
EOF
)"
```

---

### Task 3: 友好名真相源可被 runtime 复用

**Files:**
- Inspect: `apps/cli/src/tui/view_model/state.rs`、`apps/cli/src/tui/view_model/tool_name.rs`
- Modify or Create: 共享映射（优先下沉到 `packages/sdk` 或 `agent/shared` 已有位置；**NEVER** 在 runtime 复制字面量表）

- [ ] **Step 1: 定位现有 `TOOL_DISPLAY_NAMES` / `tool_display_name`，列出可下沉路径**

```bash
rg -n 'TOOL_DISPLAY_NAMES|fn tool_display_name' apps/cli packages agent/shared -g '*.rs'
```

- [ ] **Step 2: 写失败测试 — runtime 侧调用共享函数得到 `Run`（Bash）**

- [ ] **Step 3: 下沉或暴露共享 API，改 TUI 调用点改用共享源**

- [ ] **Step 4: `cargo test -p runtime` 与相关 cli 单测过滤通过后 commit**

---

### Task 4: Bash 保持；Agent `description` → `goal`

**Files:**
- Modify: `agent/features/tools/src/domain/types/agent.rs`
- Modify: Agent adapter / prompt / i18n 引用 `description` 处
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/agent.rs`（及 tests）

- [ ] **Step 1: 写失败测试 — AgentInput 序列化字段名为 `goal`；旧 JSON `description` 仍能反序列化**

- [ ] **Step 2: 改类型字段 + `serde(alias = "description")`；全仓编译错误清单化**

```bash
cargo check -p tools -p cli 2>&1 | rg 'description' | head -40
```

- [ ] **Step 3: 修调用面；AgentDisplay header 读 `goal`，Details 放 `prompt`**

- [ ] **Step 4: 跑 Agent / tools 相关测试 GREEN 后 commit**

---

### Task 5: 为其余内置工具 Input 补必填 `goal`

**Files:**
- Modify: `agent/features/tools/src/domain/types/{read,write,edit,grep,glob,web_*,tool_search,memory,task_*,ask_user,enter_worktree,exit_worktree,background_processes,skill 相关}.rs`
- Modify: 对应 adapters、prompt、i18n、fixture JSON

工具清单（与 `TOOL_DISPLAY_NAMES` 对齐，缺一不可）：
Bash(已有)、Glob、Grep、Read、Write、Edit、Agent、Skill、WebFetch、WebSearch、ToolSearch、EnterWorktree、ExitWorktree、AskUserQuestion、Task* 全族、BackgroundProcess* 全族、Memory* 全族。

- [ ] **Step 1: 为每个尚无 `goal` 的 Input 写 serde/构造失败测试（缺字段或空 goal 行为按设计）**

- [ ] **Step 2: 逐文件补 `pub goal: String` 并修编译**

- [ ] **Step 3: 更新工具描述 / guidance，明确 goal 必填且先于技术参数**

- [ ] **Step 4: `cargo test -p tools --lib` GREEN 后 commit（可按工具族拆多个 commit，但仍同一 PR）**

---

### Task 6: 全 ToolDisplay — Header 含 goal，Details 放载荷

**Files:**
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/*.rs`
- Modify: `apps/cli/src/tui/render/output/tool_display/task_impls.rs`
- Modify: 各 `*_tests.rs`

- [ ] **Step 1: 写失败测试矩阵 — 每个 Display：`format_header` 含 goal 文本；`format_details` 含载荷且不含把 goal 当唯一行**

- [ ] **Step 2: 按工具族改 `format_header` / `format_details`**

- [ ] **Step 3: 旧快照缺 goal 的 fallback 单测（每族至少 1 个）**

- [ ] **Step 4: `cargo test -p cli --bin aemeath tool_display`（或更窄过滤）GREEN 后 commit**

---

### Task 7: Background* TUI 映射（Watch/Status/List）

**Files:**
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/background_process.rs`
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/background_process_tests.rs`
- Modify: `apps/cli/src/tui/render/output/tool_display/constants.rs`（短尾常量）

- [ ] **Step 1: 写失败测试 — Watch header 形如含 summary 与短尾；Status result 含 summary；List 行末为语义 summary**

```rust
assert!(header.contains("跑 ping"));
assert!(header.contains("bgp_") || header.contains("…")); // 短尾
assert!(!header.ends_with("bgp_07dYn1F3S6a") || header.contains(" · ")); // 不再是「仅 id」
```

- [ ] **Step 2: 实现 header/result 映射；复用 `format_call_duration` 等现有 helper**

- [ ] **Step 3: GREEN 后 commit**

---

### Task 8: L4 scenario — 普通工具 goal + Background 映射

**Files:**
- Modify/Create: `apps/cli/src/tui/app/scenario_tests/` 下场景（优先扩展现有 chat/tool 场景文件）

- [ ] **Step 1: 写失败 L4 — 某工具（如 Grep）tool card 屏幕含 goal，Details 区含 query**

- [ ] **Step 2: 写失败 L4 — 后台任务 summary 为 `Run · …`，Watch/List 场景可见**

- [ ] **Step 3: 接线使场景 GREEN**

- [ ] **Step 4: commit**

---

### Task 9: L5 PTY — 全内置工具展示矩阵 + Watch 映射

**Files:**
- Modify: `apps/cli/tests/background_process_display_pty.rs`（Watch header/summary 断言）
- Create or Modify: `apps/cli/tests/tool_display_goal_pty.rs`（或等价 table-driven L5）
- Modify: slow-test-matrix / `scripts/check-slow-test-matrix.sh` 登记

- [ ] **Step 1: 列出 `TOOL_DISPLAY_NAMES` 全键，写成 L5 覆盖清单注释**

- [ ] **Step 2: 写失败 PTY 断言 — Watch 等待中/结束后 header 含语义 summary**

- [ ] **Step 3: 写失败 PTY/矩阵 — 每个内置工具至少一次屏幕出现其 goal 字符串**

- [ ] **Step 4: mock LLM 多步 tool_use 带 goal；实现至 GREEN**

- [ ] **Step 5: 登记 slow-test-matrix 并本地跑该 L5**

```bash
cargo test -p cli --test background_process_display_pty -- --ignored --nocapture
# 以及新 L5 测试二进制
```

- [ ] **Step 6: commit**

---

### Task 10: specs / runtime 设计文档同步

**Files:**
- Modify: `specs/3.3-tui-cli.md`
- Modify: `specs/3.5-tools.md`
- Modify: `docs/design/02-modules/runtime/09-background-processes.md`

- [ ] **Step 1: 在 3.3 写入 Header/Details/goal 规则与 Background* 展示表行**

- [ ] **Step 2: 在 3.5 写入「内置工具 Input MUST 含 goal」**

- [ ] **Step 3: 在 09-background-processes.md 将 summary 口径改为 `{友好名} · {goal}`**

- [ ] **Step 4: commit**

---

### Task 11: 门禁自检与开 PR

- [ ] **Step 1: 对照 Issue #1964 checklist 逐项打勾或记录理由**

```bash
gh issue view 1964 --repo rushsinging/aemeath
```

- [ ] **Step 2: 跑窄测汇总**

```bash
cargo test -p tools --lib
cargo test -p runtime --lib invocation_summary
cargo test -p cli --bin aemeath background_process
cargo test -p cli --bin aemeath tool_display
```

- [ ] **Step 3: push 并创建 PR（`Closes #1964`），Title/Body 按仓库 PR 模板**

```bash
git push -u origin HEAD
gh pr create --title "feat(tui): #1964 全工具 goal 与 Background summary 映射" --body "..."
```

- [ ] **Step 4: 等 CI 绿；用户授权后再 `/merge`**

---

## 执行方式选择

计划就绪后请选择：

1. **Subagent-Driven（推荐）** — 每任务新 subagent，任务间主 agent 复核
2. **Inline Execution** — 本会话按 `executing-plans` 批次推进

**若选 1：** 使用 `superpowers:subagent-driven-development`  
**若选 2：** 使用 `superpowers:executing-plans`
