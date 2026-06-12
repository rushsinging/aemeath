<!-- Migrated from: docs/feature/active.md#8 -->
### #8 Memory 系统

**目标**：跨会话持久化记忆，让 agent 在不同会话间积累项目知识、用户偏好和决策上下文，避免每次从零开始。

**存储设计**：

```
~/.aemeath/memory/
├── _global.json          # 全局记忆（跨项目）
├── <project-hash>/       # 项目级记忆
│   ├── _index.json       # 记忆索引（id → metadata）
│   ├── <id>.json         # 单条记忆
│   └── _archive/         # 过期/合并后的归档
```

**记忆条目结构**：

```rust
struct MemoryEntry {
    id: String,             // UUIDv7
    category: MemoryCategory,
    content: String,        // 记忆正文
    source: String,         // 来源：session id / reflection / user
    project: Option<String>,// 项目标识（None = 全局）
    relevance_tags: Vec<String>,  // 检索标签
    created_at: u64,
    accessed_at: u64,       // 最后一次被检索注入的时间
    access_count: u32,      // 被检索次数（用于优先级排序）
    expires_at: Option<u64>,// 过期时间（None = 永久）
}
```

**分类**：

```rust
enum MemoryCategory {
    ProjectStructure,  // 项目架构、文件组织
    Decision,          // 重要设计决策及其理由
    Preference,        // 用户偏好（语言、风格、框架选择等）
    Pattern,           // 项目特定模式（命名规范、错误处理方式）
    Pitfall,           // 已知坑点/踩坑记录
    Context,           // 一般上下文知识
}
```

**写入时机**：

| 时机 | 入口 | 写入策略 |
|------|------|---------|
| 用户主动 | `/memory add <content>` 命令 | 直接写入 |
| LLM 主动 | `Memory` tool | 搜索/管理长期记忆；system prompt 不再注入详细记忆内容 |
| 反思系统 | `/reflect` / 自动反思 | 生成建议；`auto_apply_suggestions=true` 时自动写入 |
| 压缩前 | Compact 前运行时流程 | 基于即将被压缩的 early messages 提取 memory 建议 |

**检索策略**：

1. System prompt 只保留 Memory tool 使用提示，不再注入 `# Project Memory` 详细内容。
2. LLM 需要长期记忆时通过 `Memory` tool 的 `search/list` 主动检索。
3. `/memory search` 仍可由用户显式检索。
4. MemoryStore 仍保留 `top_for_inject` 等评分能力，但不用于 system prompt 自动注入。

**新增模块**：

- `aemeath-core/src/memory.rs` — MemoryStore（CRUD + 索引 + 检索 + 淘汰）
- `aemeath-core/src/command/commands/memory.rs` — `/memory` 命令

**新增命令**：

| 命令 | 说明 |
|------|------|
| `/memory` | 显示当前项目的 memory 摘要 |
| `/memory add <content>` | 添加一条记忆 |
| `/memory search <query>` | 搜索记忆 |
| `/memory delete <id>` | 删除一条记忆 |
| `/memory clear` | 清空项目记忆 |

**淘汰策略**：
- 单条记忆超过 90 天未被访问（`accessed_at`）且 `access_count < 3` → 归档
- 单项目记忆超过 100 条 → 触发合并：将相近 tag 的记忆用 LLM 合并为一条摘要
- 归档文件不删除，可通过 `/memory search` 搜索

**配置**（`config.json`）：

```json
{
   "memory": {
     "enabled": true,
     "max_entries": 100,
     "similarity_threshold": 0.8,
     "reflection": {
       "enabled": true,
       "interval_turns": 10,
       "auto_apply_suggestions": false
     }
   }
}
```

**依赖**：无外部依赖，纯文件系统存储 + JSON 序列化。

#### 完成度评估（2026-05-19）：待确认

**已实现**：

| 组件 | 位置 |
|------|------|
| 存储层 `MemoryStore`（CRUD + 搜索 + 去重 + 归档） | `memory/store.rs` |
| `MemoryEntry` 结构（id/layer/category/tags/pin/ttl/outdated/access_count） | `memory/entry.rs` |
| 分类：Fact/Decision/Preference/Pattern/Pitfall | `memory/entry.rs` |
| 两层存储：Global + Project | `memory/store.rs` |
| 去重（Jaccard 相似度） | `memory/dedup.rs` |
| 评分（injection_score + eviction_score） | `memory/scoring.rs` |
| System Prompt Memory tool 提示（不注入详细记忆） | `prompt/build/prompt_build.rs` |
| `/memory` 命令（add/delete/pin/search/compact/stats） | `command/commands/memory.rs` |
| `MemoryTool`（LLM 可通过 tool call 操作 memory） | `memory_tool.rs` |
| `SessionReminders`（会话级提醒） | `memory/session_reminder.rs` |
| TUI/REPL 对话结束后展示 session reminder recap，`/clear` 清空 | `tui/app/update/reminder_recap.rs`、`repl/mod.rs` |
| 配置（`MemoryConfig` + `ReflectionConfig`） | `config/memory.rs` |

**暂缓/未实现**：

| 需求 | 说明 |
|------|------|
| `auto_summary_on_session_end` | 已删除；SessionEnd 不做 LLM 总结，不会阻塞退出 |
| `ReflectionGenerated` Hook 事件 | spec 中提到的 hook 事件不在 `HookEvent` 枚举中 |
| PostCompact 提取记忆 | 不再作为目标；已改为 Compact 前基于即将被压缩的 early messages 提取建议 |
| 淘汰策略定时触发 | 有 `compact()` + `eviction_candidates()`，但无定时触发逻辑，`archive_after_days` 配置项不在 `MemoryConfig` 中 |
| LLM 合并相近记忆 | spec 中"超过 100 条时用 LLM 合并"未落地 |
| SessionReminders 持久化 | `SessionReminders` 仅内存态，不写入文件，会话结束即丢失 |

---
