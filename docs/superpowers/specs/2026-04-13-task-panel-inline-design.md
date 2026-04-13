# 任务面板设计：输出流内嵌 + 实时更新

## 背景

将 TodoRun（代码控制的批量派发）改为 LLM 控制后，用户在 TUI 中失去了对任务执行状态的可见性。需要一个轻量的任务状态展示机制。

## 设计决策

- **位置**：输出流内嵌（作为系统消息），不新增独立 UI 区域
- **时机**：关键节点展示完整列表 + 轮询展示单条变更
- **实现**：复用 `UiEvent::SystemMessage`，不新增 UiEvent 类型

## 展示格式

### 完整列表快照

在 TaskCreate 创建任务后、TaskUpdate 标记 completed 后触发：

```
━━━ Tasks: 2/5 completed ━━━
  ✓ #1 Review aemeath-core
  ■ #2 Review aemeath-llm (@agent-1)
  □ #3 Review aemeath-tools (blocked by #1)
  □ #4 Review aemeath-cli
  □ #5 编写汇总报告 (blocked by #3, #4)
```

状态图标：
- `✓` completed
- `■` in_progress
- `□` pending

附加信息：
- `(@owner)` 显示任务 owner
- `(blocked by #X, #Y)` 显示依赖

### 单条变更消息

轮询检测到状态变化时插入：

```
  ■ #2 Review aemeath-llm — started
  ✓ #3 Review aemeath-tools — completed
```

## 实现方式

### 1. 完整快照触发

在 `process_in_background` 中，工具执行结果返回后：

- 检查 `tool_name` 是否为 `TaskCreate`
- 检查 `tool_name` 是否为 `TaskUpdate` 且输出包含 `"completed"`
- 如果匹配，从 `TaskStore` 读取所有非 deleted 任务，格式化为完整快照
- 通过 `tx.send(UiEvent::SystemMessage(snapshot))` 发送

### 2. 实时轮询

扩展现有的 timer 轮询机制（当前在 Agent/TodoRun 运行时启用 2 秒轮询）：

- 当 TaskStore 中存在 pending 或 in_progress 任务时启动轮询
- 每 2 秒检查所有任务状态，与上次快照对比
- 只对有变化的任务发送单条系统消息
- 所有任务完成或无活跃任务时停止轮询

### 3. TaskCreate tool result 简化

去掉之前添加的任务列表摘要（避免与快照重复），恢复为简短输出：

```
Task #1 created successfully: Review core modules [normal]
Description: ...
```

## 涉及文件

| 文件 | 改动 |
|------|------|
| `aemeath-cli/src/tui/app.rs` | process_in_background 中添加快照触发和轮询扩展 |
| `aemeath-tools/src/task_create.rs` | 去掉任务列表摘要，恢复简短输出 |
| `aemeath-cli/src/tui/output_area.rs` | 恢复 task tool 的默认显示行数 |

## 不做的事

- 不新增独立 UI 区域/组件
- 不修改 ratatui 布局
- 不增加快捷键
- 不新增 UiEvent 类型
