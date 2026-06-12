# Feature #8 Memory 系统（重新设计）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/114

**日期**：2026-05-01
**状态**：设计中
**依赖**：无外部依赖

## 目标

跨会话持久化记忆，让 agent 在不同会话间积累项目知识、用户偏好和决策上下文，避免每次从零开始。记忆作为一等公民，和 Messages、Tasks 并列。

## 核心设计决策

1. **在 aemeath-core 原生实现**（非 superpowers 插件）
2. **分阶段实施**：Phase 1 核心存储 + 手动管理 → Phase 2 自动注入 + Reminder Recap → Phase 3 反思系统
3. **存储两层**：Global（跨项目）+ Project（项目级），无 Session 层
4. **时效策略**：访问频率排序沉底 + 可选 TTL + pinned 保护
5. **LLM 自主管理**：MemoryTool 让 LLM 主动读写长期记忆和会话提醒
6. **Hook 兜底暂缓**：不在本轮实现 SessionEnd/PostCompact 自动提取，避免和反思系统职责重叠
7. **自动淘汰 + 用户确认暂缓**：超限时先保留归档能力，用户确认流程后续单独实现
8. **上限可配置**：`memory.max_entries`（默认 100）

---

## 数据模型

### MemoryEntry

```rust
struct MemoryEntry {
    id: String,              // UUIDv7
    layer: MemoryLayer,      // Global / Project
    category: MemoryCategory, // Fact / Decision / Preference / Pattern / Pitfall
    content: String,         // 记忆正文（≤500 字）
    source: MemorySource,    // LLM / Hook / User
    source_ref: Option<String>, // 来源引用（session id / hook event）
    tags: Vec<String>,       // 检索标签
    pinned: bool,            // 永不过期，不参与淘汰
    ttl: Option<Duration>,   // 可选过期时间
    created_at: u64,         // Unix timestamp
    accessed_at: u64,        // 最后被注入的时间
    access_count: u32,       // 被注入次数
    outdated: bool,          // 反思系统标记为过时（降低评分但不删除）
}

enum MemoryLayer { Global, Project }
enum MemoryCategory { Fact, Decision, Preference, Pattern, Pitfall }
enum MemorySource { LLM, Hook, User }
```

### SessionReminders（纯内存，不持久化）

用户在当前会话中的待办/提醒，对话结束后以 recap 形式展示给用户。

```rust
struct SessionReminder {
    id: String,
    content: String,
    done: bool,
    created_at: u64,
}
```

- **不注入 system prompt**，LLM 看不到
- **每轮对话结束后**，TUI 底部以 `* recap: xxx | yyy` 形式展示给用户
- 会话结束时不持久化，直接丢弃
- LLM 通过 MemoryTool 的 `add_reminder` / `complete_reminder` 管理
- 用户可通过 `/memory remind` 查看完整列表
- `/clear` 时清空 reminders

---

## 存储格式

### 文件结构

```
~/.aemeath/memory/
├── _global.json                # 全局活跃记忆
├── _global_archive.json        # 全局归档记忆
├── <project-hash>.json         # 项目活跃记忆
├── <project-hash>_archive.json # 项目归档记忆
```

每个文件是一个 JSON 数组：

```json
[
  {
    "id": "01923abc-...",
    "layer": "project",
    "category": "pattern",
    "content": "错误处理统一用 AemeathError + thiserror derive",
    "source": "hook",
    "source_ref": "session:01923def",
    "tags": ["error", "rust"],
    "pinned": false,
    "ttl": null,
    "created_at": 1746000000,
    "accessed_at": 1746100000,
    "access_count": 5,
    "outdated": false
  }
]
```

### 分层规则

| 层 | 存储位置 | 生命周期 | 注入范围 |
|---|---------|---------|---------|
| Global | `_global.json` | 永久 | 所有会话 |
| Project | `<project-hash>.json` | 永久 | 该项目下所有会话 |

---

## MemoryStore API

```rust
pub struct MemoryStore {
    base_dir: PathBuf,        // ~/.aemeath/memory/
    project_hash: String,     // 当前项目 hash
    max_entries: usize,       // 配置上限
}

impl MemoryStore {
    // --- CRUD ---
    pub fn add(&mut self, entry: MemoryEntry) -> Result<AddResult, MemoryError>;
    pub fn delete(&mut self, id: &str) -> Result<(), MemoryError>;
    pub fn update(&mut self, id: &str, content: &str) -> Result<(), MemoryError>;
    pub fn pin(&mut self, id: &str, pinned: bool) -> Result<(), MemoryError>;
    pub fn mark_outdated(&mut self, id: &str) -> Result<(), MemoryError>;

    // --- 检索 ---
    pub fn search(&self, query: &str, limit: usize) -> Vec<&MemoryEntry>;

    // --- 注入（构建 system prompt 时调用）---
    pub fn top_for_inject(&mut self, limit: usize) -> Vec<&MemoryEntry>;
    // 混合 Global + Project 两层，按评分排序取 top-N
    // 更新 accessed_at / access_count，批量写回

    // --- 淘汰 ---
    pub fn needs_eviction(&self, layer: MemoryLayer) -> bool;
    pub fn eviction_candidates(&self, layer: MemoryLayer, count: usize) -> Vec<MemoryEntry>;
    pub fn archive_entries(&mut self, ids: &[String]) -> Result<(), MemoryError>;
    pub fn compact(&mut self) -> Result<CompactResult, MemoryError>;  // /memory compact

    // --- 去重 ---
    fn find_similar(&self, content: &str, threshold: f64) -> Option<&MemoryEntry>;
    // Jaccard 相似度，不做 embedding

    // --- 工具方法 ---
    pub fn stats(&self) -> MemoryStats;
    pub fn list(&self, layer: Option<MemoryLayer>) -> Vec<&MemoryEntry>;
}

pub enum AddResult {
    Added,
    Merged { existing_id: String },
    NeedsEviction { candidates: Vec<MemoryEntry> },
}

pub struct CompactResult {
    pub archived: usize,
    pub remaining: usize,
}

pub struct MemoryStats {
    pub global_count: usize,
    pub global_archive_count: usize,
    pub project_count: usize,
    pub project_archive_count: usize,
    pub reminders_count: usize,
}
```

---

## 检索排序

注入时排序公式（从高到低取 top-N）：

```
score = (if pinned { 10000 } else { 0 })
      + min(access_count, 20) * 100      // 访问次数权重，上限 20
      + recency_score(accessed_at)        // 0-1000，越近越高
      - (if ttl_expired { 5000 } else { 0 })  // 已过期排到最后
      - (if outdated { 2000 } else { 0 })     // 过时标记降权
```

**recency_score 分段衰减**：

| 时间范围 | 得分 |
|---------|------|
| 1 天内 | 1000 |
| 7 天内 | 800 |
| 30 天内 | 500 |
| 90 天内 | 200 |
| 90 天以上 | 50 |

---

## 注入

在 `prompt.rs` 的 `build_system_prompt_parts` 中，`dynamic_part` 末尾追加：

```
# Project Memory
- [Decision] 使用 uuidv7 做 session id，替换随机 hex
- [Pattern] 错误处理统一用 AemeathError + thiserror derive
- [Pitfall] bash.rs check_command_safety 不受 allow_all 控制
- [Preference] 用户偏好中文回复、简洁风格
```

**注入规则**：
- Global 层记忆始终参与排序
- Project 层记忆按当前 cwd 的 project-hash 匹配参与排序
- 两层混合排序后取 top-`max_inject_count`（默认 10）
- 注入后更新每条的 `accessed_at` 和 `access_count`，批量写回
- 归档文件不参与注入

---

## MemoryTool（LLM 自主管理）

注册为 aemeath-tools 中的工具：

```rust
pub struct MemoryTool;

impl Tool for MemoryTool {
    fn name(&self) -> &str { "Memory" }
    fn description(&self) -> &str {
        "管理长期记忆。支持 add/delete/search/pin/list/add_reminder/complete_reminder 操作。
         记忆跨会话持久化，在后续会话中自动注入 system prompt。
         用于记录重要决策、用户偏好、项目约定、踩坑经验等。
         Session Reminders 用于记录当前会话的待办提醒，会话结束后展示给用户。"
    }
}
```

**操作类型**：

| 操作 | 参数 | 说明 |
|------|------|------|
| `add` | layer, category, content, tags, pinned, ttl_hours | 添加记忆 |
| `delete` | id | 删除记忆 |
| `search` | query | 搜索记忆 |
| `pin` | id, pinned | 固定/取消固定 |
| `list` | layer | 列出记忆 |
| `add_reminder` | content | 添加会话提醒 |
| `complete_reminder` | id | 完成会话提醒 |

**LLM 写入时机（自主判断）**：
- 用户明确表达偏好 → `Memory add category=preference`
- 做出重要设计决策 → `Memory add category=decision`
- 发现坑点或踩坑 → `Memory add category=pitfall`
- 用户要求"记住这个" → `Memory add`
- 发现之前记忆过时 → `Memory delete id=xxx` 或重新 add
- 用户说"等下处理这个" → `Memory add_reminder`

---

## Hook 兜底（暂缓）

本轮不实现 Hook 兜底自动提取。原因：

1. SessionEnd/PostCompact 自动提取会和 Feature #9 反思系统的 suggested_memories 职责重叠。
2. PostCompact 后再提取只看到压缩摘要，可能已经损失关键上下文，不适合作为首选记忆来源。
3. Hook 输出协议、用户确认策略、冲突处理都需要单独设计，避免在 Memory MVP 中扩大范围。

后续如恢复该方向，优先考虑**压缩前**或**会话自然结束前**基于完整上下文生成候选，再交由用户确认或 reflection 的 `auto_apply_suggestions` 策略处理。

## 去重策略

`add()` 内部实现去重：

1. 计算新内容与现有记忆的 Jaccard 相似度（关键词交集比例）
2. 相似度 ≥ 0.8 → 合并（保留 tags 更多的，更新 accessed_at），返回 `AddResult::Merged`
3. 不相似 → 检查是否需要淘汰
4. 需要淘汰 → 返回 `AddResult::NeedsEviction { candidates }`，由调用方弹出 AskUserQuestion 让用户选择
5. 不需要 → 直接写入

**相似度算法**：Jaccard（关键词集合交集 / 并集），不做 embedding。记忆条目短（≤500字），关键词匹配足够。

---

## 自动淘汰

**触发条件**：活跃条目数 ≥ `max_entries` 时触发。

**淘汰流程**：
1. 计算所有活跃条目的淘汰评分（pinned 的排除）
2. 按评分从低到高排序，取最低分的 N 条作为候选
3. 弹出 AskUserQuestion 展示候选列表，让用户选择归档哪些
4. 用户确认后，选中的移入 `_archive.json`
5. 新记忆写入

**淘汰评分公式**：
```
score = (if pinned { +∞ })       // 永不淘汰
      + access_count * 10
      + recency_days 的倒数
```

**用户确认交互**：
```
📝 记忆已满（100/100），建议归档以下记忆：

[1] [Pitfall] bash.rs check_command_safety 不受 allow_all 控制 (最后访问: 30天前, 引用: 2次)
[2] [Context] 项目使用 tokio 1.x runtime (最后访问: 45天前, 引用: 1次)

选择要归档的记忆：
a) 全部归档
b) 保留部分
```

---

## Session Reminders 展示

**每轮对话结束后**，TUI output area 末尾追加：

```
* recap: 待处理 /clear 命令 bug（#25）| 待测试 Zhipu 重试逻辑
```

- 已完成的 reminder 不展示（或灰显划线）
- 用户可通过 `/memory remind` 查看完整列表
- `/clear` 时清空所有 reminders

---

## 命令

| 命令 | 说明 |
|------|------|
| `/memory` | 显示所有记忆（分 Global/Project 两层）+ Session Reminders |
| `/memory add <content>` | 手动添加记忆 |
| `/memory delete <id>` | 删除记忆 |
| `/memory pin <id>` | 固定记忆（不参与淘汰） |
| `/memory search <query>` | 搜索记忆（含归档） |
| `/memory compact` | 主动触发淘汰归档 |
| `/memory remind` | 查看当前会话 Reminders |
| `/memory stats` | 显示记忆统计 |

---

## 配置

```json
{
  "memory": {
    "enabled": true,
    "max_entries": 100,
    "max_inject_count": 10,
    "auto_summary_on_session_end": true,
    "similarity_threshold": 0.8
  }
}
```

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `enabled` | bool | true | 是否启用记忆系统 |
| `max_entries` | usize | 100 | 每层活跃记忆上限 |
| `max_inject_count` | usize | 10 | 注入 system prompt 的最大条数 |
| `auto_summary_on_session_end` | bool | true | 会话结束是否自动提取记忆 |
| `similarity_threshold` | f64 | 0.8 | 去重相似度阈值 |

---

## 新增文件

```
aemeath-core/src/
├── memory/
│   ├── mod.rs               # MemoryStore + MemoryEntry + MemoryLayer
│   ├── scoring.rs           # 排序/淘汰评分算法
│   ├── dedup.rs             # 去重逻辑（Jaccard 相似度）
│   └── session_reminder.rs  # SessionReminders（纯内存）
aemeath-core/src/config/
│   └── memory.rs            # MemoryConfig 配置结构
aemeath-tools/src/
│   └── memory_tool.rs       # MemoryTool（LLM tool call）
aemeath-core/src/command/commands/
│   └── memory.rs            # /memory 命令族
```

---

## 分阶段实施

### Phase 1 — 核心存储 + 手动管理

- `memory/mod.rs`：MemoryStore（CRUD、文件读写）
- `memory/scoring.rs`：排序算法
- `memory/dedup.rs`：去重逻辑
- `memory/session_reminder.rs`：SessionReminders
- `/memory` 命令族（memory.rs）
- MemoryTool（add/delete/search/pin/add_reminder/complete_reminder）
- 配置：`memory.rs`
- TUI：output area 末尾 recap 展示

### Phase 2 — 自动注入 + Reminder Recap

- system prompt 注入 Memory（prompt.rs 改造），并使用 `MemoryConfig` 的 `max_entries`、`similarity_threshold`、`max_inject_count`
- 每轮对话结束后展示 Session Reminders recap
- `/memory remind` 查看当前会话 reminders
- `/memory compact` 命令保留手动归档能力

### Phase 2 暂缓项

- SessionEnd Hook 自动提取记忆
- PostCompact Hook 提取
- 淘汰归档的 AskUserQuestion 确认流程

### Phase 3 — 反思系统（见 Feature #9）

### Phase 4 — 打磨

- 记忆导入/导出（`/memory export` / `/memory import`）
- 跨项目记忆合并
- 记忆统计面板（`/memory stats`）
