# 06 — Tool::goal_text 与 Watch 短 header

> 关联：Issue #1964（milestone v0.2.0）  
> 状态：已定稿（2026-10-13 纠偏）  
> 关联：`specs/3.3-tui-cli.md`、`specs/3.5-tools.md`、`docs/design/02-modules/runtime/09-background-processes.md`

## 1. 问题

BackgroundProcessWatch 用户面 header 过长或不直观。根因：

1. 转后台写入的 `invocation_summary` 是 `tool=… input=…` 机械拼接；
2. Watch 展示优先挂完整 task_id / 冗长友好名，未把账本 summary 收成短 header。

曾尝试「全内置工具 schema 必填 goal」——**已否决**：多数工具意图即主载荷，强制字段是重复税（PR #1970 已关并撤回）。

## 2. 目标与非目标

### 目标

1. 每个 `Tool` / `TypedTool` 提供 **`goal_text(input) -> String`**；
2. **有非空 `goal` 字段 → 返回该字段**；否则按该工具当前 header/载荷语义拼一串；
3. 转后台 `invocation_summary = goal_text(...)`；
4. Watch header：**`Watch · {summary} · {短尾}`**。

### 非目标

- **NEVER** 全员 schema `required: ["goal"]`；
- 不改 task_id 形态；
- 不强制所有普通工具卡片 Header 改版（可 follow-up）；
- MCP：有 `goal` 则用，否则截断参数（非硬门禁）。

## 3. 术语

| 术语 | 含义 |
|---|---|
| **goal 字段** | 部分工具 Input 已有的可选/既有意图字段（如 Bash.`goal`、Agent.`description`） |
| **goal_text** | Tool 方法：从一次调用 input 抽出「一句话意图/摘要」 |
| **summary** | 账本 `BackgroundProcessSummaryData.summary` = 被转后台那次调用的 `goal_text` |
| **短尾** | task_id 截断（保留前缀可辨 + 末尾），常量进 `tool_display/constants.rs` |

## 4. 决策表

| ID | 决策 |
|---|---|
| D1 | `Tool`/`TypedTool` 增 `goal_text`；默认实现：非空 `goal` → 用之；否则按工具名拼载荷 |
| D2 | Agent 无 `goal` 字段时用既有 `description`（本 issue **不**强制改名） |
| D3 | `ToolExecutionPort::goal_text` 默认合成；`ExecutionAdapter` 委派真实 `Tool::goal_text`（runtime **NEVER** 拿 `Arc<dyn Tool>`） |
| D4 | `invocation_summary_text` = `execution.goal_text(tool_name, input)` |
| D5 | Watch 友好名在 header 用 **`Watch`**；形态 `Watch · {summary} · {短尾}`；无 summary 时 `Watch · {短尾}` |
| D6 | 单 PR；L2+L4+L5 覆盖 summary 与 Watch header |

## 5. 架构

```
LLM tool_use { …input }
        │
        ├─► Tool::goal_text(input)  ──┐
        │                             │
        └─► (转后台)                  ▼
              ToolExecutionPort::goal_text
                    → record.invocation_summary
                    → BackgroundProcessSummaryData.summary
                    → Watch header: Watch · {summary} · {短尾}
```

### 5.1 `goal_text` 默认合成（无 goal 字段时）

按工具名选取主载荷（与当前 TUI header 同源语义，截断防爆）：

| 工具 | 合成 |
|---|---|
| Bash | `command` 截断 |
| Agent | `description` |
| Grep | `/{pattern}/, path={path}` |
| Glob | `pattern` |
| Read/Write/Edit | `file_path` |
| WebFetch | `url` |
| WebSearch / ToolSearch | `query` |
| Memory* / Task* / Background* | 既有主字段（id/query/subject/task_id…） |
| 其他 / MCP | 首个非空字符串字段或截断 JSON |

### 5.2 Watch TUI

| 状态 | Header |
|---|---|
| 等待中（尚无 typed result） | `Watch · {短尾}` |
| 有 WatchResult | `Watch · {detail.summary.summary} · {短尾}` |

List/Status result 行继续展示 summary 字段即可；本 issue 不强制改 Status header 长友好名。

## 6. 测试

| 层 | 内容 |
|---|---|
| L0/L1 | `goal_text`：有 goal / 无 goal 合成 / Agent description |
| L2 | `invocation_summary_text` 不再含 `tool=`/`input=`；经 port 调用 |
| L4 | Watch header 含 `Watch ·` + summary + 短尾（无完整冗长 id） |
| L5 | 扩展 `background_process_display_pty`：断言 Watch 短 header 与 summary |

## 7. 文档同步

- `specs/3.5-tools.md`：`goal_text` 契约
- `specs/3.3-tui-cli.md`：Watch header 形态
- `09-background-processes.md`：summary = goal_text

## 8. 验收清单

与 Issue #1964 checklist 对齐；PR 使用 `Closes #1964`。
