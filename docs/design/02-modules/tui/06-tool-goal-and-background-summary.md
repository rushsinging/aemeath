# 06 — 工具 `goal` 契约与 BackgroundProcess summary / TUI 映射

> 关联：Issue #1964（milestone v0.2.0）  
> 状态：已定稿（2026-10-13）  
> 关联既有：`specs/3.3-tui-cli.md`、`specs/3.5-tools.md`、`docs/design/02-modules/runtime/09-background-processes.md`

## 1. 问题

BackgroundProcessWatch 用户面 header 只显示 `Backend Process Watch bgp_…`，不直观。
根因不在 Watch 单独缺字段，而在：

1. 多数工具缺少与 Bash `goal` 同级的**调用意图**字段；
2. 转后台写入的 `invocation_summary`（→ `BackgroundProcessSummaryData.summary`）是 `tool=… input=…` 机械拼接；
3. Background* 展示优先消费 task_id，未把 summary 提到 header。

## 2. 目标与非目标

### 目标

- **所有内置 tool call**（含 Agent）统一必填语义字段 `goal`。
- **TUI 统一卡片规则**：Header 含友好名 + goal；技术载荷进 **Details**。
- 后台账本 `summary = "{友好名} · {goal}"`，List / Status / Watch 全族消费。
- Watch header：`Backend Process Watch · {summary} · bgp_短尾`。
- **L4 scenario + L5 PTY** 覆盖**全部内置工具**展示，以及 Background* 映射。

### 非目标

- 不改 task_id 形态 / typed id 前缀。
- 不改 status bar「N Backend Progress」计数文案（可 follow-up）。
- 不把整段 command / prompt / 大 query 塞进 header。
- 不在本 issue 重做 MCP 动态工具的完整 schema 治理（见 §6 边界）。

## 3. 术语

| 术语 | 含义 | 例子 |
|---|---|---|
| **友好名** | `TOOL_DISPLAY_NAMES` 映射后的固定显示名 | `Run`、`Agent`、`Backend Process Watch` |
| **goal** | 本次调用意图，模型必填 | `跑 ping 验收` |
| **Details** | `DetailsPolicy::Expanded` 行：技术载荷 | Bash `command`、Agent `prompt` |
| **summary** | 账本字段 `BackgroundProcessSummaryData.summary` | `Run · 跑 ping 验收` |
| **outcome** | Watch 本次等待结局 | `still_running` / `terminal` / `cancelled` |
| **state** | 后台任务生命周期状态 | `backgrounded` / `succeeded` / … |

`outcome` 与 `state` **不重复**：前者描述 Watch 等得怎样，后者描述被监视任务处在哪一档。

## 4. 定稿决策

| # | 决策 | 值 |
|---|---|---|
| D1 | 语义字段 | 全内置工具统一必填 `goal` |
| D2 | Agent | 现有 `description` **迁为 `goal`**；serde 接受旧键 `description` 作别名，写入新会话一律 `goal` |
| D3 | Header | `{友好名} {goal}`（goal 空仅允许旧快照 fallback，生产路径 NEVER 故意留空） |
| D4 | Details | 技术载荷：command / prompt / query+path / file_path / url 等 |
| D5 | 账本 summary | `{友好名} · {goal}`（友好名用 `tool_display_name`，与 TUI 一致） |
| D6 | Watch header | `Backend Process Watch · {summary} · {task_id 短尾}` |
| D7 | Status / List | Status header 对齐 Watch（summary + 短尾）；List 行保留 id/tool/state，末段用 summary；Status result **MUST** 含 summary |
| D8 | 交付 | 单 PR |
| D9 | 验收 | L4 + L5 覆盖全部内置 ToolDisplay 注册工具 |

## 5. 架构

```
LLM tool_use { goal, …payload }
        │
        ├─► ToolDisplay::format_header  → "{友好名} {goal}"
        │   ToolDisplay::format_details → payload 行
        │
        └─► (若转后台) execution_supervisor::invocation_summary_text
                → record.invocation_summary = "{友好名} · {goal}"
                → BackgroundProcessSummaryData.summary
                → List / Status / Watch 展示
```

### 5.1 Schema（tools）

- 每个内置 `*Input` **MUST** 含 `pub goal: String`（必填；空串视为非法，adapter/校验侧拒绝或按既有校验风格报错）。
- Agent：字段改名 `goal`；`#[serde(alias = "description")]` 兼容旧 transcript。
- Bash：已有 `goal`，保持。
- BackgroundProcess* / Task* / Memory* / Web* / filesystem / search / Skill / AskUserQuestion / Enter|ExitWorktree：一律补 `goal`。
- Prompt / i18n / model guidance：**MUST** 同步要求模型先填 goal。

### 5.2 摘要写入（runtime）

替换 `invocation_summary_text`：

1. 从 `call.input`（完整 JSON，而非仅 preview）解析 `goal`；
2. 友好名 = `tool_display_name(tool_name)`（runtime 侧需可调用的共享映射，或 tools/share 下沉同一真相源，**NEVER** 在 runtime 再抄一份字面量表而不共享）；
3. 输出 `"{友好名} · {goal}"`；
4. goal 缺失（旧会话/损坏）fallback：`"{友好名} · (no goal)"`，**NEVER** 再回退整段 `input=` JSON。

### 5.3 TUI（Background*）

| 工具 | Header | Result |
|---|---|---|
| List | `Backend Processes`（无单任务） | `{id} · {tool} · {state} · {summary}` |
| Status | `Backend Process · {summary} · {短尾}` | 含 state / deadline / log bytes / **summary** |
| Watch | `Backend Process Watch · {summary} · {短尾}` | log_tail + 末行 `outcome · id · state · duration · summary` |
| Logs / Stop | 同族：能解析到 summary 的走 summary+短尾；否则仅 id（Logs/Stop 结果本身无 summary 字段时可只改 header 若 Status 已查过——首版：**header 仍可先显示 id**，优先保证 List/Status/Watch） |

短尾规则：保留 typed id 前缀语义可辨即可（例如最后 8 字符或 `bgp_…` 截断到固定宽度）；具体常量进 `tool_display/constants.rs`。

### 5.4 普通工具卡片

所有已注册 `ToolDisplay`：

- `format_header`：**MUST** 读 `goal`；
- `format_details`：**MUST** 输出技术载荷（不得把 goal 再抄进 Details）；
- 旧快照缺 goal：fallback 到既有字段（Bash→command 截断、Agent→旧 description、Grep→query…），并在单测锁死 fallback 仅服务兼容。

## 6. MCP / 动态工具边界

- 内置工具：本 issue **MUST** 全覆盖。
- MCP 动态工具：首版 **MAY** 在通用 MCP ToolDisplay / 执行封装上要求或注入 `goal`；若 MCP JSON Schema 无法强制，TUI header fallback 为服务器工具名 + 截断参数，并在验收清单标明「MCP 非本 issue 硬门禁」。

## 7. 测试策略（L0–L5）

参照 `docs/design/03-engineering/04-testing-and-coverage.md`。

| 层 | 内容 |
|---|---|
| L0/L1 | 各 `*Input` serde：缺 `goal` 失败或默认空+校验失败；Agent `description` 别名可读 |
| L2 | `invocation_summary_text` 单测；各 `ToolDisplay` header/details 单测 |
| L3 | tools adapter 对空 goal 的行为（若有校验） |
| L4 | `apps/cli/src/tui/app/scenario_tests`：注入含 goal 的 tool_start/result，断言 header/details；Background* 映射场景（转后台后 List/Watch 可见 `友好名 · goal`） |
| L5 | 扩展 `background_process_display_pty` + **全工具展示矩阵**（可 table-driven / 多 mock 响应）：每个内置工具至少一次屏幕断言含 goal；Watch 断言 header 含 summary 与短尾 |

**L5 全工具**：允许一个 PTY 测试文件内分 section，或 `tool_display_goal_pty` 新文件 + slow-test-matrix 登记；**NEVER** 豁免 L5。

## 8. 文档同步

同 PR 更新：

- `specs/3.3-tui-cli.md`：Header/Details/`goal`/Background* 展示表
- `specs/3.5-tools.md`：输入契约 `goal` 必填
- `docs/design/02-modules/runtime/09-background-processes.md`：summary 口径改为语义摘要
- 本文件保持为定稿真相源

## 9. 风险

| 风险 | 缓解 |
|---|---|
| 旧 session / 旧 tool_use 缺 goal | serde 兼容 + TUI fallback；新调用强制 |
| 友好名映射双份 | 下沉/复用单一 `tool_display_name` 真相源 |
| L5 全工具体量大 | table-driven + slow matrix；按工具族分断言块 |
| Agent 字段改名破坏外部 | `serde(alias="description")` |

## 10. 验收清单（门禁）

与 Issue #1964 checklist 对齐；PR 使用 `Closes #1964`。
