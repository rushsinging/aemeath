# Feature #8/#9 Memory 与 Reflection 实施计划

**日期**：2026-05-01  
**范围**：Feature #8 Memory 系统、Feature #9 反思系统  
**依据**：`docs/feature/specs/008-memory-system.md`、`docs/feature/specs/009-reflection-system.md`  
**总体策略**：先交付可用的 Memory 核心闭环，再接入自动注入与 Hook 兜底，最后实现 Reflection。Feature #9 严格依赖 Feature #8 的存储、工具和注入能力。

---

## 1. 总体目标

### 1.1 Memory 系统

实现跨会话持久化记忆，使 agent 能长期保存并复用：

- 项目事实与结构知识
- 用户偏好
- 架构/实现决策
- 项目特定模式
- 已知坑点
- 当前会话内的临时 reminders

Memory 作为一等能力进入：

- core 数据模型
- 文件存储
- slash command
- LLM tool
- system prompt 注入
- Hook 兜底提取

### 1.2 Reflection 系统

在关键节点回顾近期行为和已有 Memory，发现偏差、遗漏和过期记忆，并将有价值的经验写回 Memory。

Reflection 输出进入 output area，不放进 status bar。

---

## 2. 关键设计决策

1. **Memory 原生实现在 `aemeath-core`**，不是 superpowers/skill 插件。
2. **存储两层**：Global + Project；不引入 Session 持久层。
3. **Reflection 依赖 Memory**：反思建议最终写入 MemoryStore。
4. **文件存储优先**：纯 JSON 文件，无外部数据库/embedding 依赖。
5. **先手动、后自动**：先实现 `/memory` 和 MemoryTool，再接入自动注入、Hook、Reflection。
6. **用户确认优先**：淘汰归档、Reflection 建议写入默认需要用户确认。
7. **单文件不超过 400 行**：Memory 与 Reflection 拆模块实现。

---

## 3. 分阶段计划

## Phase 0：准备与配置建模

### 目标

为 Memory/Reflection 建立配置入口、模块骨架和错误类型。

### 修改文件

- `aemeath-core/src/config/mod.rs`
- 新增 `aemeath-core/src/config/memory.rs`
- `aemeath-core/src/lib.rs`
- 新增 `aemeath-core/src/memory/mod.rs`
- 新增 `aemeath-core/src/memory/error.rs`

### 任务

1. 新增 `MemoryConfig`：
   - `enabled: bool = true`
   - `max_entries: usize = 100`
   - `max_inject_count: usize = 10`
   - `auto_summary_on_session_end: bool = true`
   - `similarity_threshold: f64 = 0.8`
   - `reflection: ReflectionConfig`

2. 新增 `ReflectionConfig`：
   - `enabled: bool = true`
   - `interval_turns: usize = 10`
   - `auto_apply_suggestions: bool = false`
   - `model: Option<String> = None`

3. 在 `Config` 加字段：
   - `pub memory: MemoryConfig`

4. 新增 Memory 错误类型：
   - 文件读写失败
   - JSON 序列化失败
   - id 不存在
   - 配置非法
   - 输入非法

### 验收

- `cargo test -p aemeath-core config::memory`
- `cargo check -p aemeath-core`
- 配置文件缺省时能反序列化默认值。

---

## Phase 1：Memory 核心存储

### 目标

实现不依赖 LLM/TUI 的 MemoryStore，完成 CRUD、搜索、排序、去重、归档。

### 新增文件

```text
aemeath-core/src/memory/
├── mod.rs
├── entry.rs
├── store.rs
├── scoring.rs
├── dedup.rs
├── session_reminder.rs
└── error.rs
```

### 数据模型

实现：

- `MemoryEntry`
- `MemoryLayer::{Global, Project}`
- `MemoryCategory::{Fact, Decision, Preference, Pattern, Pitfall}`
- `MemorySource::{Llm, Hook, User}`
- `AddResult`
- `CompactResult`
- `MemoryStats`

注意：serde 命名建议统一用 `snake_case` 或 `lowercase`，并在测试中固定格式，避免后续配置/JSON 兼容问题。

### 存储路径

```text
~/.aemeath/memory/
├── _global.json
├── _global_archive.json
├── <project-hash>.json
└── <project-hash>_archive.json
```

### project hash

建议：

- 对 canonical cwd 做稳定 hash。
- 输出短 hex，避免路径泄漏进文件名。
- 测试中允许传入临时 base_dir 和 project_hash，避免污染用户目录。

### MemoryStore API

实现：

- `new(base_dir, project_hash, max_entries, similarity_threshold)`
- `add(entry) -> AddResult`
- `delete(id)`
- `update(id, content)`
- `pin(id, pinned)`
- `mark_outdated(id)`
- `search(query, limit)`
- `list(layer)`
- `stats()`
- `top_for_inject(limit)`
- `needs_eviction(layer)`
- `eviction_candidates(layer, count)`
- `archive_entries(ids)`
- `compact()`

### 搜索策略

Phase 1 采用简单文本匹配：

- content 包含 query
- tags 包含 query
- category/layer 可参与匹配
- 返回按注入 score 排序

### 排序策略

注入 score：

```text
score = pinned_bonus
      + min(access_count, 20) * 100
      + recency_score(accessed_at)
      - ttl_expired_penalty
      - outdated_penalty
```

淘汰 score：

```text
score = access_count * 10 + recency_weight
```

pinned 永不淘汰。

### 去重策略

- Jaccard 相似度。
- similarity >= `similarity_threshold` 时合并。
- 合并时：
  - 保留旧 id。
  - 合并 tags。
  - 更新 accessed_at/access_count。
  - content 默认保留更长或旧内容，避免无提示覆盖用户写入内容。

### SessionReminders

纯内存，不落盘：

- `add(content)`
- `complete(id)`
- `list()`
- `clear()`
- `recap_line()`

### 测试要求

核心逻辑必须单测覆盖：

- add 正常写入 Global/Project
- add 相似内容触发 merge
- delete 不存在 id 返回错误
- pin 后不进入 eviction candidates
- outdated 降低注入排名
- top_for_inject 更新 accessed_at/access_count
- archive_entries 从 active 移到 archive
- search 覆盖 content/tags/category
- SessionReminders add/complete/clear/recap

### 验收

- `cargo test -p aemeath-core memory`
- `cargo check -p aemeath-core`

---

## Phase 2：`/memory` 命令族

### 目标

用户可以手动管理 Memory 和当前会话 Reminders。

### 修改文件

- 新增 `aemeath-core/src/command/commands/memory.rs`
- 修改 `aemeath-core/src/command/commands/mod.rs`
- 可能修改 `CommandAction`，视命令是否需要 TUI 特殊处理。

### 命令

```text
/memory
/memory add <content>
/memory delete <id>
/memory pin <id>
/memory search <query>
/memory compact
/memory remind
/memory stats
```

### 实现策略

1. `/memory`：显示 Global/Project 两层 active memory 摘要 + reminders。
2. `/memory add <content>`：默认写 Project 层，category 初期可用 `Fact`。
3. `/memory delete <id>`：删除 active memory。
4. `/memory pin <id>`：切换 pinned。
5. `/memory search <query>`：搜索 active + archive。
6. `/memory compact`：触发归档候选；需要确认时返回 Confirm 或交给 TUI AskUserQuestion。
7. `/memory remind`：列出当前会话 reminders。
8. `/memory stats`：显示统计。

### 设计注意

当前 `CommandContext` 没有 MemoryStore，需要决定注入方式：

- 方案 A：在 `AppState` 持有 MemoryStore/SessionReminders。
- 方案 B：命令执行时根据 config/cwd 临时打开 MemoryStore。

建议：

- 持久 MemoryStore 可按需打开，避免 AppState 生命周期复杂化。
- SessionReminders 必须在 TUI/REPL 会话状态中持有，因为它不持久化。

### 测试要求

- 命令解析正常路径。
- 参数缺失返回中文错误。
- delete/pin/search 的错误路径。

### 验收

- `/memory add 测试记忆` 后 `/memory search 测试` 能找到。
- `cargo test -p aemeath-core command::commands::memory`
- `cargo check -p aemeath-cli`

---

## Phase 3：MemoryTool

### 目标

让 LLM 主动读写 Memory。

### 修改文件

- 新增 `aemeath-tools/src/memory_tool.rs`
- 修改 `aemeath-tools/src/lib.rs`
- 可能修改 tool 初始化参数，让 MemoryTool 获得 cwd/config/session reminder 状态。

### Tool 名称

```text
Memory
```

### 操作

| action | 参数 |
|---|---|
| add | layer, category, content, tags, pinned, ttl_hours |
| delete | id |
| search | query, limit |
| pin | id, pinned |
| list | layer |
| add_reminder | content |
| complete_reminder | id |

### 安全与约束

- content 限制 <= 500 字符。
- tags 数量和长度限制。
- 非法 category/layer 返回结构化错误。
- delete/pin 不允许误删 archive，Phase 1 只处理 active。
- 所有用户可见错误用中文。

### Tool 注册

在：

```text
aemeath-tools/src/lib.rs
```

将 MemoryTool 注册到：

- `register_all_tools`
- `register_all_tools_except_agent`

但需要先解决构造依赖：MemoryTool 需要知道当前 cwd/project_hash/base_dir/config。若现有工具注册时拿不到这些信息，可先让 MemoryTool 每次执行时从环境/当前进程 cwd 推导；更好的方案是扩展注册函数参数。

### 测试要求

- add/search/list 正常路径。
- 无效 action 错误路径。
- 超长 content 边界。
- add_reminder/complete_reminder。

### 验收

- LLM tool schema 中出现 Memory。
- 手动调用 tool 能写入 `~/.aemeath/memory/<project-hash>.json`。
- `cargo test -p aemeath-tools memory_tool`

---

## Phase 4：TUI/REPL SessionReminders 展示

### 目标

每轮对话结束后，在 output area 末尾展示当前未完成 reminder recap。

### 修改文件

- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/app/stream.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/output_area/mod.rs`
- `aemeath-cli/src/repl/mod.rs`

### 展示格式

```text
* recap: 待处理 /clear 命令 bug（#25）| 待测试 Zhipu 重试逻辑
```

### 行为

- 每轮 assistant 完成后刷新 recap。
- 已完成 reminders 不展示。
- `/clear` 清空 reminders。
- `/memory remind` 展示完整列表。

### 验收

- MemoryTool `add_reminder` 后一轮结束显示 recap。
- `/clear` 后 recap 消失。
- `cargo check -p aemeath-cli`

---

## Phase 5：System Prompt 注入 Memory

### 目标

构建 system prompt 时注入 top-N Memory。

### 修改文件

- `aemeath-cli/src/prompt.rs`
- 可能新增 `aemeath-core/src/memory/inject.rs`

### 当前入口

```text
aemeath-cli/src/prompt.rs::build_system_prompt_parts
```

### 实现

1. `build_system_prompt_parts(cwd, hook_runner)` 中读取 MemoryConfig。
2. 若 `memory.enabled == true`：
   - 根据 cwd 计算 project_hash。
   - 打开 MemoryStore。
   - 调用 `top_for_inject(max_inject_count)`。
   - 将结果追加到 dynamic part。
3. 格式：

```text
# Project Memory
- [Decision] ...
- [Pattern] ...
- [Pitfall] ...
- [Preference] ...
```

### 注意

- Global 与 Project 混排。
- archive 不注入。
- 注入后批量更新 accessed_at/access_count。
- 注入内容需要长度上限，避免污染上下文。
- Memory 为空时不添加标题。

### 测试要求

- 无 Memory 时 dynamic 不包含 `# Project Memory`。
- 有 Global/Project Memory 时按 score 注入。
- 注入后 access_count 增加。
- disabled 时不注入。

### 验收

- 启动后系统提示中包含 Memory。
- `cargo test -p aemeath-cli prompt`
- `cargo check -p aemeath-cli`

---

## Phase 6：Hook 兜底提取

### 目标

在会话结束和压缩后自动提取重要信息写入 Memory。

### 修改文件

- `aemeath-core/src/hook.rs`
- `aemeath-core/src/config/hooks.rs`
- `aemeath-cli/src/tui/app/stream.rs`
- `aemeath-cli/src/repl/mod.rs`
- 可能新增内置 hook 辅助模块，或直接在 core 中提供 Memory extractor。

### 当前注意点

现有 Hook 里有 SessionStart/Stop/Compact 相关数据，但 specs 里提到 `SessionEnd`。需要先统一事件模型：

- 若已有 `Stop` 表达会话停止，则决定是否复用 Stop。
- 若需要新增 `SessionEnd`，要同步：
  - `HookEvent`
  - `HookData`
  - `HookRunner` 方法
  - config 反序列化测试
  - TUI/REPL 调用点

建议：

- Feature #8 使用现有 `Stop` 做第一版兜底，避免新增重叠事件。
- 如果产品语义要求区分「一轮停止」和「会话结束」，再新增 `SessionEnd`。

### PostCompact

当前搜索未看到 `PostCompact` 调用，需要确认 compact 流程中是否已触发。若没有：

1. 在 compact 前调用 `PreCompact`。
2. compact 后调用 `PostCompact`。
3. `CompactHookData` 需要包含：
   - turns
   - messages_before
   - messages_after
   - was_compacted
   - 可选 compacted_summary（若要给 Memory 提取使用）

### 自动提取策略

Phase 6 可先不做额外 LLM 调用，采用 Hook 脚本或内置轻量规则。完整方案：

1. 会话结束时收集 session summary。
2. 调用轻量 LLM 输出候选 MemoryEntry JSON。
3. 去重后写入 Project 层。
4. 超限则走 AskUserQuestion 确认归档。

### 测试要求

- Stop/SessionEnd hook 能写入 Memory。
- PostCompact hook 能收到 compact 数据。
- Hook stdin/env 包含 event/project_dir。
- 提取失败不影响主流程，只记录日志。

### 验收

- 退出会话后 Memory 文件出现 Hook source 条目。
- compact 后关键摘要可进入 Memory。
- `cargo test -p aemeath-core hook memory`
- `cargo check -p aemeath-cli`

---

## Phase 7：Reflection 核心模块

### 目标

实现可手动触发的 Reflection 引擎和输出模型。

### 新增文件

```text
aemeath-core/src/reflection/
├── mod.rs
└── prompt.rs
```

### 数据模型

实现：

- `ReflectionOutput`
- `MemorySuggestion`
- `ReflectionEngine`
- `ReflectionError`

### Prompt

`prompt.rs` 提供函数：

- `build_reflection_prompt(project_memory, recent_summary)`
- 输出要求 JSON。

### 输入

- 当前 Project Memory 全量或限长后的内容。
- 最近 N 轮对话摘要。

### 输出处理

- `deviations`：展示给用户。
- `suggested_memories`：默认 pending，不直接写入。
- `outdated_memories`：调用 `MemoryStore::mark_outdated`。
- `user_alert`：展示给用户。

### 测试要求

- ReflectionOutput JSON 反序列化。
- malformed JSON 返回中文错误。
- prompt 包含 memory 和 recent_summary。
- outdated memory 标记成功。

### 验收

- `cargo test -p aemeath-core reflection`
- `cargo check -p aemeath-core`

---

## Phase 8：`/reflect` 命令与展示

### 目标

用户可手动触发 Reflection，并在 output area 查看结果。

### 修改文件

- 新增 `aemeath-core/src/command/commands/reflect.rs`
- 修改 `aemeath-core/src/command/commands/mod.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/output_area/mod.rs`

### 命令

```text
/reflect
/reflect apply
/reflect stats
/reflect history
```

Phase 8 只必须实现：

- `/reflect`
- `/reflect apply`

stats/history 可放 Phase 10。

### 展示格式

```text
─── Reflection ───
偏差检测：
  - ...

建议记忆：
  - [Decision] ... (+)

过时记忆：
  - ...

用户提示：...
────────────────
```

### 用户确认

- `/reflect` 产生 suggestions，暂存在 App 状态。
- `/reflect apply` 将 pending suggestions 写入 MemoryStore。
- 若 `auto_apply_suggestions = true`，直接写入。

### 验收

- `/reflect` 能展示反思结果。
- `/reflect apply` 能写入 Memory。
- `cargo check -p aemeath-cli`

---

## Phase 9：自动 Reflection 触发

### 目标

接入 N 轮自动触发和 PostCompact 触发。

### 修改文件

- `aemeath-cli/src/tui/app/stream.rs`
- `aemeath-cli/src/repl/mod.rs`
- compact 相关模块

### 行为

1. 每完成一轮对话，turn_count + 1。
2. 若 `turn_count % interval_turns == 0`，后台触发 Reflection。
3. Reflection 结果追加到 output area。
4. PostCompact 后触发一次 Reflection，检查压缩是否遗漏上下文。
5. 失败只显示简短提示并写日志，不打断用户。

### 并发限制

- 同一时间只允许一个 Reflection 任务。
- 用户正在输入时不要抢占输入框。
- Reflection LLM 调用要有 timeout。

### 验收

- 配置 `interval_turns = 2`，两轮后自动出现 Reflection。
- compact 后自动 Reflection。
- `auto_apply_suggestions=false` 时不会自动写 Memory。

---

## Phase 10：打磨与归档

### Memory 打磨

- `/memory export`
- `/memory import`
- 更好的列表格式
- archive 搜索优化
- memory stats 面板

### Reflection 打磨

- `/reflect stats`
- `/reflect history`
- 建议采纳率统计
- 对重复建议去重

### 文档/追踪

- Feature #8/#9 完成后先将状态改为“待确认”。
- 等用户确认后再从 `docs/feature/active.md` 移除并归档。

---

## 4. 依赖关系

```text
Phase 0
  ↓
Phase 1 MemoryStore
  ↓
Phase 2 /memory 命令 ───────┐
  ↓                         │
Phase 3 MemoryTool          │
  ↓                         │
Phase 4 Reminders UI        │
  ↓                         │
Phase 5 Prompt 注入         │
  ↓                         │
Phase 6 Hook 兜底           │
  ↓                         │
Phase 7 Reflection 核心 ←───┘
  ↓
Phase 8 /reflect + 展示
  ↓
Phase 9 自动触发
  ↓
Phase 10 打磨归档
```

---

## 5. 风险与待决策

### 5.1 SessionEnd vs Stop

当前实现中已有 Stop hook，spec 中写 SessionEnd。需要确认：

- Stop 是否表示单轮 agent 停止？
- SessionEnd 是否表示整个 CLI/TUI 退出？

若语义不同，应新增 `SessionEnd`，否则 Feature #8 第一版复用 Stop。

### 5.2 MemoryTool 状态注入

现有 `register_all_tools` 参数没有 MemoryStore/config/cwd。需要决定：

- 扩展工具注册参数。
- 或 MemoryTool 每次执行时自行根据 cwd/config 打开 store。

建议第一版选择“按需打开 store”，减少对工具注册架构的侵入。

### 5.3 Reflection LLM 调用位置

Reflection 需要独立轻量 LLM 调用。需要决定放在：

- `aemeath-core` 只构造 prompt 和解析输出，实际调用在 `aemeath-cli/aemeath-llm`。
- 或 core 依赖 llm client。

建议保持 core 不依赖 llm，ReflectionEngine 只做 prompt/parse/process，调用由 CLI 层协调。

### 5.4 AskUserQuestion 集成

Memory 淘汰和 Reflection apply 都需要用户确认。当前 Tool AskUserQuestion 与 TUI 命令确认路径需要统一，否则可能出现 TUI/REPL 行为不一致。

### 5.5 文件并发写

MemoryStore 文件 JSON 读写存在并发风险。第一版可用进程内 Mutex；长期应考虑原子写：

- 写临时文件
- fsync/flush
- rename 替换

---

## 6. 最小可交付切片

如果要尽快落地，建议先做以下 MVP：

1. `MemoryConfig`
2. `MemoryEntry + MemoryStore add/search/list/delete`
3. `/memory add/search/list/delete`
4. `MemoryTool add/search/list`
5. system prompt 注入 top 10

暂缓：

- 自动淘汰确认
- Hook 自动总结
- Reflection
- import/export
- stats/history

MVP 完成后即可获得跨会话记忆基本能力。

---

## 7. 验证命令

每个阶段至少运行：

```text
cargo test -p aemeath-core memory
cargo test -p aemeath-core config
cargo check -p aemeath-core
cargo check -p aemeath-cli
```

涉及 tools 后运行：

```text
cargo test -p aemeath-tools memory_tool
cargo check -p aemeath-tools
```

最终集成：

```text
cargo check
```

---

## 8. Feature 状态更新建议

开始实施时：

- #8：`重新设计中` → `实施中`
- #9：保持 `重新设计中`，直到 #8 Phase 5 完成后再进入 `实施中`

完成代码但未用户确认时：

- #8/#9：`待确认`

用户确认后：

- 从 `docs/feature/active.md` 移除对应条目。
- 归档到 `docs/feature/archived/`。
