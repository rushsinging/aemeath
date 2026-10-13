# 06 — ToolCall::goal_text 与 Watch · {goal}

> 关联：Issue #1964（milestone v0.2.0）  
> 状态：已定稿（2026-10-13 ToolCall 模型）  
> 关联：`specs/3.3-tui-cli.md`、`specs/3.5-tools.md`、`docs/design/02-modules/tools/01-domain-model.md`、`docs/design/02-modules/runtime/09-background-processes.md`

## 1. 问题

1. Tool 对象模型贫血：catalog 里的 `BashTool` 是**定义单例**，一次调用的 `input` 不在对象上，`goal_text(&self, input)` 只能 externally 传参。
2. Watch header 叠 id / 等 typed result 才有意图，职责漏到 TUI。

## 2. 拍板

| ID | 决策 |
|---|---|
| D1 | 肥在 **`ToolCall`（调用态）**；`BashTool` 等保持定义单例 |
| D2 | `ToolCall::goal_text(&self) -> String`（读 `self.input`） |
| D3 | Runtime 仍只经 `ToolExecutionPort`；adapter 内绑 Definition |
| D4 | Watch header：有 goal → `Watch · {goal}`；无 → 完整 `bgp_id` |
| D5 | 扩 #1964，M1–M4 一次做完 |

## 3. 数据流

```
Bash 转后台:
  ToolCall { tool_name: Bash, input }.goal_text()
    → Record.invocation_goal

Watch 展示前（runtime）:
  task_id → 账本.invocation_goal
    → 注入 ToolCallStateChanged.arguments.goal
    → TUI 只渲染 Watch · {goal}
```

## 4. 命名

- 账本：`invocation_goal`（serde alias `invocation_summary`）
- DTO：`BackgroundProcessSummaryData.goal`（serde alias `summary`）
- Published Language：`ToolCall`（`ToolInvocation` = 类型别名兼容）

## 5. 非目标

- Runtime 持有 `Arc<dyn Tool>`
- Catalog 单例挂载某次 input
- TUI 查账本 / 等 typed result
- 全员 schema 必填 goal

## 6. 验收

- ToolCall::goal_text(&self) 单测
- Watch **等待中** header = `Watch · {goal}`（input 已含 runtime 注入的 goal）
- 无 goal → 完整 task_id
- header NEVER 叠短尾 id
