# Feature #9 反思系统（重新设计）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/115

**日期**：2026-05-01
**状态**：设计中
**依赖**：Feature #8 Memory 系统（反思的输出目标）

## 目标

在关键节点自动触发反思，让 agent 回顾已有记忆和近期行为，主动发现偏差并修正，提炼经验写入 Memory 系统，避免重复犯错。

## 核心设计决策

1. **依赖 #8**：反思结果写入 MemoryStore，使用 MemoryTool 的 API
2. **反思结果追加在 output area 末尾**（而非 status bar）
3. **建议的记忆默认需用户确认**，可配置自动应用
4. **N 轮自动触发 + 手动触发**两种时机；不做 PostCompact 后反思，避免压缩后上下文损失

---

## 反思触发时机

| 时机 | 触发方式 | 说明 |
|------|---------|------|
| N 轮对话后 | Agent 主循环计数 | 每 N 轮自动触发（默认 10 轮，可配置） |
| 用户主动 | `/reflect` 命令 | 手动触发 |

**不做 PostCompact 后反思**：压缩完成后只剩压缩摘要，关键上下文可能已经丢失；如果后续需要与 compact 联动，应优先设计为 compact 前反思或基于完整上下文生成候选。

---

## 反思流程

```
触发反思
  → 检索当前 Project Memory（全部，不截断）
  → 取最近 N 轮对话摘要
  → 注入 ReflectPrompt 到 LLM（独立轻量调用）
  → LLM 输出 ReflectionOutput
  → 处理结果：
     - deviations → 展示给用户（output area）
     - suggested_memories → 用户确认后写入 MemoryStore
     - outdated_memories → 标记为 outdated（降低评分但不删除）
     - user_alert → 展示给用户
```

---

## 反思数据模型

```rust
pub struct ReflectionOutput {
    /// 发现的偏差（和已有决策/偏好不一致的行为）
    deviations: Vec<String>,
    /// 建议新增的记忆
    suggested_memories: Vec<MemorySuggestion>,
    /// 建议标记为过时的记忆 ID
    outdated_memories: Vec<String>,
    /// 给用户的重要提示
    user_alert: Option<String>,
}

pub struct MemorySuggestion {
    category: MemoryCategory,
    content: String,
    tags: Vec<String>,
    reason: String,  // 为什么建议添加这条记忆
}
```

---

## 反思 Prompt

作为独立 LLM 调用注入，使用轻量模型：

```
你是一个反思助手。请回顾以下信息，分析是否存在偏差或遗漏：

## 当前项目记忆
{project_memory 全量}

## 最近对话摘要
{recent_summary}

请检查：
1. 近期行为是否和已有决策/偏好一致？如有偏差，列出 deviations
2. 是否有新的经验/模式值得记录？如有，建议新增记忆
3. 已有记忆中是否有过时内容？如有，建议标记为过时
4. 是否需要提醒用户？如有重要发现，写 user_alert

输出 JSON 格式的 ReflectionOutput。
```

---

## 反思结果处理

```rust
impl AgentState {
    pub async fn handle_reflection(&mut self, output: ReflectionOutput) {
        // 1. 标记过时记忆（降低评分，不删除）
        for id in &output.outdated_memories {
            self.memory_store.mark_outdated(id)?;
        }

        // 2. 展示反思摘要到 output area
        if !output.deviations.is_empty()
            || !output.suggested_memories.is_empty()
            || output.user_alert.is_some()
        {
            self.show_reflection_summary(output);
        }
    }
}
```

### show_reflection_summary → 追加到 output area 末尾

```
─── Reflection ───
⚠ 偏差检测：
  - 当前会话使用了 print_stdout，但项目偏好是使用日志系统

💡 建议记忆：
  - [Decision] Memory 模块采用方案 C，记忆作为一等公民 (+)
  - [Pattern] 会话结束 Hook 自动提取记忆 (+)

📤 过时记忆：
  - "项目使用 Rust 1.75" → 建议更新

用户提示：项目 preference 记录偏好中文错误消息，但刚才的 PR 写了英文注释
────────────────
```

### 用户交互

- **`(+)` 标记**：建议新增的记忆旁显示 `(+)`，用户可点击/选择确认添加
- **忽略**：不操作，建议不写入
- **`/reflect apply`**：一次性应用所有建议

---

## 配置

```json
{
  "memory": {
    "reflection": {
      "enabled": true,
      "interval_turns": 10,
      "auto_apply_suggestions": false,
      "model": null
    }
  }
}
```

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `enabled` | bool | true | 是否启用反思系统 |
| `interval_turns` | usize | 10 | 每 N 轮自动触发反思 |
| `auto_apply_suggestions` | bool | false | 反思建议的记忆是否自动写入（false 需用户确认） |
| `model` | Option<String> | null | 反思使用的模型（null = 使用当前会话模型） |

---

## 新增文件

```
aemeath-core/src/
├── reflection/
│   ├── mod.rs          # 反思引擎 + ReflectionOutput
│   └── prompt.rs       # 反思 Prompt 模板
aemeath-core/src/command/commands/
│   └── reflect.rs      # /reflect 命令
```

---

## 分阶段实施

### Phase 1（依赖 #8 Phase 1 完成）

- `reflection/mod.rs`：反思引擎 + ReflectionOutput 结构
- `reflection/prompt.rs`：Prompt 模板
- `/reflect` 命令
- 反思结果展示到 output area
- 建议记忆的用户确认流程

### Phase 2（依赖 #8 Phase 2 完成）

- N 轮自动触发（Agent 主循环计数）
- `auto_apply_suggestions` 配置支持
- 继续使用当前会话默认模型；`model = null` 表示不单独切换模型

### Phase 2 暂缓项

- PostCompact 后反思；如后续恢复，应改为 compact 前或完整上下文候选提取

### Phase 3

- 反思统计分析（`/reflect stats`：反思命中率、建议采纳率等）
- 反思历史记录（`/reflect history`）
