# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 4 | AskUserQuestion TUI 美化 | - | 待实施 | 未确认 | AskUserQuestion 向用户确认时，TUI 界面需要美化 |
| 8 | Memory 系统 | - | 重新设计中 | 未确认 | 跨会话持久化记忆，记忆作为一等公民，LLM 自主管理 + Hook 兜底。详见 [spec](specs/008-memory-system.md) |
| 9 | 反思系统 | - | 重新设计中 | 未确认 | 关键节点自动反思，发现偏差并提炼经验写入 Memory（依赖 #8）。详见 [spec](specs/009-reflection-system.md) |
| 11 | OpenAI reasoning_effort 配置支持 | - | ✅ 已完成 | 未确认 | 支持 GPT-5.x 系列 `reasoning_effort` (none/low/medium/high/xhigh)，可在 config.json 配置 |
| 13 | Task list 显示在 spinner 下方 | - | ✅ 已完成 | 未确认 | 临时区域渲染顺序调整为：queued messages → spinner → task status lines，让 task list 出现在 spinner 下方而非上方 |
| 15 | 通过 max_tokens 限制 LLM thinking 长度 | - | 待实施 | 未确认 | 用户可配置 thinking 阶段最大 token 数，避免模型在简单问题上过度思考浪费 token / 时间；映射到各 provider 的 thinking budget 字段 |
| 16 | Spinner 行合并状态显示 + Hook 调用信息 | - | 待确认 | 未确认 | 把 status line 的 Thinking / Calling xxx 等运行态搬到 output area spinner 行（如 `✻ cooking ... 2s (Thinking ...)`），并在 spinner 行展示当前 hook 调用信息 |
| 17 | Skill 延迟加载 + 命名空间前缀 | - | ✅ 已完成 | 未确认 | 启动只读 frontmatter 不读全文，Skill 工具调用时按需加载；skill 包自动加 `plugin_name:` 前缀；HookJsonOutput camelCase 反序列化修复 |
| 18 | Task list 跨轮次 batch 机制 | - | ✅ 已完成 | 未确认 | Task 跟随 session 持久化，不再每次用户消息清空；按 batch 分组显示，新 turn 自动切换到新 batch，旧 batch 隐藏；已完成 task 在当前 batch 内继续显示 |

### #17 Skill 延迟加载 + 命名空间前缀

**目标**：对齐 Claude Code 的 plugin/skill 加载机制，降低启动开销，支持 skill 包（如 superpowers）的自动发现和命名空间隔离。

**已完成的改动**：

1. **启动只读 frontmatter**：`parse_skill()` 不再读取 SKILL.md 的 body content，`Skill.content` 启动时为空字符串。新增 `read_skill_content()` 函数，由 Skill 工具调用时按需读取全文。
2. **Skill 工具延迟加载**：`aemeath-tools/src/skill_tool.rs` 调用时通过 `read_skill_content()` 从 `source_path` 读取完整内容返回给 LLM。
3. **命名空间前缀**：`load_skills_from_dir()` 自动识别 skill 包（含 `skills/` 子目录的目录），包内 skill 自动加 `<pkg_name>:` 前缀（如 `superpowers:brainstorming`），原始名保留为 alias。顶层 skill 和普通目录下的 skill 无前缀。
4. **HookJsonOutput 修复**：`aemeath-core/src/hook.rs` 的 `HookJsonOutput` 加了 `#[serde(rename_all = "camelCase")]`，修复 hook 脚本输出的 `additionalContext`（camelCase）无法被反序列化的问题。
5. **SessionStart hook 精简**：`superpowers-inject.sh` 从注入全文（~5500 字符/每次 API 调用）改为简短提示（113 字符），提醒 LLM 检查可用 skill 并通过 Skill 工具按需加载。
6. **Skill 目录扫描优化**：自动发现 skill 包内的 `skills/` 子目录，跳过 `agents/`、`.github/` 等无关目录。

**涉及路径**：
- `aemeath-core/src/skill.rs`（parse_skill 延迟加载、load_skills_from_dir 命名空间、read_skill_content）
- `aemeath-tools/src/skill_tool.rs`（Skill 工具调用时读取全文）
- `aemeath-core/src/hook.rs`（HookJsonOutput camelCase 支持）
- `~/.aemeath/hooks/superpowers-inject.sh`（SessionStart hook 精简）

**测试**：7 个单元测试覆盖命名空间前缀、延迟加载、忽略非 skills 目录、常规 skill 目录。

---

### #18 Task list 跨轮次 batch 机制

**目标**：Task list 跨轮次持久化，不再每次用户消息清空。通过 batch 机制区分不同 turn 的 task list，旧 batch 自动隐藏。

**已完成的改动**：

1. **移除自动清空**：`stream.rs` 不再在每次进入时调用 `_task_store.clear()`，task 跟随 session 生命周期。
2. **Batch ID 机制**：`Task` 新增 `batch` 字段，`TaskStore` 新增 `current_batch` 计数器。`create()` 时检测上一 batch 是否全部 completed/deleted，如果是则递增 batch。
3. **当前 batch 显示**：新增 `list_current_batch()` 方法，TUI 只显示最新 batch 的 task（含 Completed）。
4. **Completed 可见**：当前 batch 内 Completed 的 task 继续显示（✓ 图标），摘要行 `━━ Tasks: 3/5 ━━` 反映完成进度。

**涉及路径**：
- `aemeath-core/src/task.rs`（batch 字段、current_batch 计数器、list_current_batch）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 使用 list_current_batch）
- `aemeath-cli/src/tui/app/stream.rs`（移除 clear 调用）

---

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

**写入时机**（通过 Hook 触发）：

| 时机 | HookEvent | 写入策略 |
|------|-----------|---------|
| 会话结束时 | `SessionEnd` | LLM 总结本会话关键决策和发现，写入 memory |
| 压缩后 | `PostCompact` | 提取被压缩掉的重要上下文到 memory |
| 用户主动 | `/memory add <content>` 命令 | 直接写入 |
| 反思系统 | `ReflectionGenerated`（新事件） | 反思结果写入 |

**检索注入**（System Prompt 构建阶段）：

1. `build_system_prompt_parts()` 中新增 memory 检索步骤
2. 基于当前 cwd 定位项目 memory 目录
3. 按 `access_count` + `created_at` 加权排序，取 top-N（默认 10 条）
4. 注入到 system prompt 的 dynamic_part 中：
   ```
   # Project Memory
   - [Decision] 使用 tokio channel 而非 mpsc，因为需要跨 async task 通信
   - [Pattern] 错误处理统一用 AemeathError，thiserror derive
   - [Pitfall] bash.rs 中 check_command_safety 不受 allow_all 控制，已修复
   ```
5. 更新被注入条目的 `accessed_at` 和 `access_count`

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
    "max_entries_per_project": 100,
    "max_inject_count": 10,
    "auto_summary_on_session_end": true,
    "archive_after_days": 90
  }
}
```

**依赖**：无外部依赖，纯文件系统存储 + JSON 序列化。

---

### #9 反思系统

**目标**：在关键节点自动触发反思，让 agent 从过去的行为中提炼经验，写入 Memory 系统，避免重复犯错。

**反思触发时机**：

| 触发点 | 条件 | 反思内容 |
|--------|------|---------|
| 连续工具失败 | 同一 turn 内 ≥2 次工具调用失败 | 失败原因分析 + 正确做法 |
| 会话结束 | `SessionEnd` hook | 整体会话总结 + 关键决策 |
| 子代理结束 | `SubagentStop` hook | 子代理执行摘要 |
| 用户中断 | 用户按 Escape 取消 | 当前进度快照 + 未完成原因 |
| 重试后成功 | API 错误后重试成功 | 错误类型 + 重试策略有效性 |

**反思流程**：

```
触发条件满足
  → 构造反思 prompt（含近期对话片段）
  → 调用 LLM 生成反思摘要（用轻量模型，如 deepseek-chat）
  → 解析反思结果为结构化 MemoryEntry
  → 写入 MemoryStore
```

**反思 Prompt 模板**：

```
你是一个反思助手。请分析以下对话片段，提炼出对未来会话有价值的信息。

要求：
1. 只记录客观事实和有效经验，不要记录临时状态
2. 每条不超过 200 字
3. 标注分类：Decision / Pattern / Pitfall / Preference

对话片段：
{recent_messages}

请输出 JSON 数组：
[{"category": "...", "content": "...", "tags": ["..."]}]
```

**反思结果结构**：

```rust
struct ReflectionResult {
    entries: Vec<ReflectionEntry>,
}

struct ReflectionEntry {
    category: MemoryCategory,
    content: String,
    tags: Vec<String>,
}
```

**实现策略**：

1. 反思调用使用**独立轻量 LLM 调用**（非主对话），避免干扰上下文
2. 反思在后台异步执行（tokio::spawn），不阻塞主循环
3. 反思结果静默写入 MemoryStore，不显示在对话中
4. 仅在 `memory.enabled = true` 且有有效反思内容时触发

**配置**（`config.json`）：

```json
{
  "reflection": {
    "enabled": true,
    "model": "deepseek/deepseek-chat",
    "max_entries_per_reflection": 3,
    "min_turns_for_session_summary": 5,
    "consecutive_failures_threshold": 2
  }
}
```

**依赖**：
- Feature #8（Memory 系统）— 反思结果写入 MemoryStore
- Hook 系统 — 通过 HookEvent 触发反思

**实施阶段**：
- P0：会话结束反思（最核心，收益最大）
- P1：连续工具失败反思
- P2：子代理反思、用户中断反思

---

### #16 Spinner 行合并状态显示 + Hook 调用信息

**目标**：把 status line 上承担的 "Thinking..." / "Calling xxx..." / "Generating..." 等运行态信息搬到 output area 的 spinner 行展示，让用户视线只需要盯一个地方就能掌握 agent 正在做什么；同时把 hook 触发信息（哪个 event 跑了哪个 hook）也并入 spinner 行展示。

**当前状态**：
- spinner 行只有 spinner 字符 + 一句固定 "Generating..."（或类似），细分阶段（thinking、calling、tool 名）写在底部 status bar 里
- 用户需要在 output area 与 status bar 之间来回扫视
- hook 调用对用户完全不可见，只能事后翻 `aemeath.log`，无法在交互中确认 hook 是否生效

**预期形态**：

spinner 行组合显示：

```
✻ cooking ... 2s (Thinking ...)
✻ cooking ... 8s (Calling Read foo.rs)
✻ cooking ... 12s (Hook PreToolUse: lint-shell.sh)
```

- 主词：`cooking` / `working`（统一占位词，配置可选）
- 计时：从当前 turn 开始的秒数
- 括号：当前细分阶段
  - `Thinking ...` — LLM thinking 阶段
  - `Calling <ToolName> <param_summary>` — 当前 tool call
  - `Hook <Event>: <hook_name>` — hook 正在执行
  - `Generating ...` — LLM 生成普通文本中
  - `Compacting ...` — 压缩阶段
  - `Waiting for user` — AskUser 等待中（spinner 暂停）

status bar 仍保留：
- 模型 / provider / cwd / token / cost 等环境与累计数据
- 不再承担"当前在做什么"的瞬时状态

**Hook 调用信息展示**：

| 事件 | spinner 行表现 |
|------|----------------|
| PreToolUse / PostToolUse / PostToolUseFailure / PreCompact / PostToolBatch / Stop / StopFailure / SessionStart | `Hook <Event>: <hook_command_short>` 持续期内显示 |
| UserPromptSubmit | 在 input area 提交后短暂显示 1 秒，再切回正常 spinner |

补充考虑：
- hook 退出后保留 1 秒"已完成"状态再切走，避免短 hook 一闪而过看不见
- hook 阻止操作（exit 2 或 `decision: block`）时，spinner 行变红色 + `Hook <Event> blocked: <reason 摘要>`
- 多个 hook 排队执行时，依次显示当前正在跑的那个，不堆栈

**实施分解**：

1. 抽取 spinner 行 model：`SpinnerLine { phase: Phase, started_at: Instant, hook: Option<HookSnapshot> }`，phase 涵盖 Thinking / Calling / Hook / Generating / Compacting / WaitingUser
2. UI 事件层：
   - 现有 `UiEvent::Thinking` / `ToolCall` / `ToolResult` 复用，但 update.rs 不再写入 status bar 文字，只更新 SpinnerLine.phase
   - 新增 `UiEvent::HookStart { event, name }` / `UiEvent::HookEnd { result }`，由 HookRunner 在执行前后发送
3. 渲染：output_area/spinner.rs 的 render 拼接 `<spinner> <main> ... <elapsed>s (<phase desc>)`
4. status_bar.rs 移除 thinking_text / tool_call_name / generating 字段，只保留环境 + 累计数据
5. 颜色：hook block / 错误用红色，hook 普通用 cyan，thinking 用 dim，calling 用 magenta，generating 用绿
6. 配置开关：`config.json` 加 `spinner.merge_status: true`（默认开），`spinner.label: "cooking"`

**与已归档 / 现有 feature 的关系**：
- Feature #1 Hook 系统已落地事件，本 feature 把 hook 运行态可视化
- Feature #13（task list 在 spinner 下方）已确定 spinner 行的相对位置，本 feature 不动布局，只改 spinner 行内容
- Bug #25（/clear 未清空 status line）：本 feature 简化了 status bar 字段，复位逻辑相应简化
- Feature #11 / #15 reasoning effort：spinner 行可在 Thinking 阶段附带 effort 等级，如 `Thinking high ...`

**涉及路径**：
- `aemeath-cli/src/tui/output_area/spinner.rs`（spinner 行 model + render）
- `aemeath-cli/src/tui/app/update.rs`（事件路由调整 + 新增 HookStart/HookEnd）
- `aemeath-cli/src/tui/app/mod.rs`（SpinnerLine 状态字段）
- `aemeath-cli/src/tui/status_bar.rs`（移除瞬时态字段）
- `aemeath-core/src/hook.rs` / `aemeath-cli/` 中 HookRunner 调用处（发送 HookStart/HookEnd UI 事件）
- `aemeath-core/src/config/`（spinner.merge_status / spinner.label 配置）

**测试场景**：
- 普通对话 → spinner 显示 Thinking → Generating → 消失
- 多 tool call → spinner 在 Calling A / Calling B / Calling C 之间切换，elapsed 持续累计
- PreToolUse hook 拦截 → spinner 红色 + "Hook PreToolUse blocked: ..." 后切回 Thinking
- 长 hook（>1s）→ spinner 期间显示 hook 名称 + elapsed
- AskUser 等待 → spinner 暂停 + "Waiting for user"
- /clear → spinner 行清空，status bar 环境信息保留

**开放问题**：
- spinner 主词 `cooking` 是否需要在用户语境下可关（一些用户可能觉得不专业）→ 默认配置 `spinner.label = "thinking"` 可能更稳，需决策
- hook 名称太长时如何截断（路径很长的 shell 脚本）
- elapsed 计时从哪里起算？建议从当前 turn 用户提交时刻起算，跨多 tool call 不重置
- 多个 hook 并行跑（PostToolBatch + PostToolUse）时显示策略：聚合显示 `Hook x2 running` 或只显示最后一个

---


**目标**：当 LLM 调用 AskUserQuestion tool call 时，TUI 中的确认界面需要美化，提升可读性和交互体验。

**当前状态**：基础功能已实现（`UiEvent::AskUser` + `update.rs` 中 `ask_user_reply_tx` 机制），但显示为普通 system message + 纯文本选项，缺乏视觉层次。

**待改进**：
- 问题文本高亮/醒目样式
- 选项列表带序号和视觉区分
- 输入提示区域样式优化

**涉及路径**：`aemeath-cli/src/tui/app/update.rs`（`UiEvent::AskUser` 处理）、`aemeath-cli/src/tui/output_area/`（渲染样式）

---

### #9 反思系统

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思流程，对最近的行为、决策、失败、用户反馈做结构化总结，将有价值的经验写入 Memory 系统（#8），让 agent 在未来会话中能够基于历史经验做更好的决策。

**依赖**：Feature #8 Memory 系统（反思的输出目标）

**设计草案**：

#### 触发时机
- **任务完成后**：TaskUpdate 将 task 置为 `completed` 时，对该 task 的执行过程做总结
- **Stop 事件**：会话结束 / agent 主动停止时，对整段会话做反思
- **错误恢复后**：tool call 失败 → 修复 → 成功 的链路上，提炼"哪种修复有效"
- **用户显式触发**：`/reflect` slash 命令，对最近 N 轮做即时反思
- **PostCompact 钩子**：上下文压缩前抢救关键经验

#### 反思维度
- **成功模式**：哪些工具组合 / 推理路径达成了目标
- **失败教训**：哪些假设错了、哪些 tool call 走了弯路
- **用户偏好**：用户在本次会话中的纠正、拒绝、确认（参考 superpowers `feedback` 类型）
- **未解决问题**：本次会话中悬而未决的事项（提示下次继续）

#### 输出格式
- 结构化条目（type / title / body / scope），写入 Memory 系统
- 每条反思 must 标注来源会话 ID + 时间戳，便于追溯
- 避免重复：写入前检索 Memory，相似条目优先 update 而非 insert

#### 实施阶段
1. **Phase 1**：实现 `/reflect` 命令 + 基础反思 prompt 模板（依赖 #8 已落地的 Memory 接口）
2. **Phase 2**：接入 Stop / TaskUpdate(completed) 自动触发
3. **Phase 3**：错误恢复链路反思 + PostCompact 钩子

**涉及路径**（待实施）：
- `aemeath-core/src/reflection/` — 反思引擎、prompt 模板、写入策略
- `aemeath-core/src/command/commands/reflect.rs` — `/reflect` 命令
- `aemeath-cli/src/tui/app/update.rs` — Stop 事件触发钩子
- `aemeath-cli/src/tui/app/stream.rs` — TaskUpdate / 错误恢复触发钩子

**开放问题**：
- 反思是否消耗当前 session 的 model 调用，还是用独立的轻量 model（成本权衡）
- 反思失败（如 LLM 返回空）时是否静默丢弃 vs 提示用户
- Memory 容量上限策略：何时压缩 / 淘汰旧反思

---

### #11 OpenAI reasoning_effort 配置支持

**目标**：支持 GPT-5.x 系列模型（GPT-5、GPT-5.2、GPT-5.4、GPT-5.5）的 `reasoning_effort` 参数，让用户能精确控制 thinking 强度（速度/质量权衡），并通过 `config.json` 持久化配置。

**背景**：
- OpenAI GPT-5.x 通过 `reasoning_effort` 控制思考深度，取值：`none` / `low` / `medium`（默认）/ `high` / `xhigh`
- 当前 aemeath 在 `openai_compatible` provider 只发 `enable_thinking: false` 兼容字段，未传 `reasoning_effort`，所以接到 GPT-5.5 时只能拿到默认 medium 行为
- 不同 reasoning provider 控制方式不同：
  - **OpenAI GPT-5.x**：`reasoning_effort: "low"|"medium"|"high"|"xhigh"|"none"` 或 Responses API 的 `reasoning: {"effort": "..."}`
  - **DeepSeek**：`thinking: {"type": "enabled"|"disabled"}`（仅 on/off）
  - **GLM/Qwen 等**：`enable_thinking: true|false`（仅 on/off）
  - **Anthropic**：`thinking: {"type": "enabled", "budget_tokens": N}`（带 budget）

**设计**：

#### 1. config.json 字段

新增模型级配置 `reasoning_effort`，与现有 `reasoning` 开关并存：

```json
{
  "models": {
    "default": "gpt-5.5",
    "list": [
      {
        "name": "gpt-5.5",
        "provider": "openai",
        "model": "gpt-5.5",
        "reasoning": true,
        "reasoning_effort": "low"
      },
      {
        "name": "deepseek-r1",
        "provider": "deepseek",
        "model": "deepseek-r1",
        "reasoning": true
      }
    ]
  }
}
```

- `reasoning_effort` 为可选字段，类型 `Option<String>`
- 取值校验：`none|low|medium|high|xhigh`，非法值启动时报错
- 仅对支持的 provider 生效（OpenAI / OpenRouter / OpenAICompatible 路由到 OpenAI 模型时）；其他 provider 收到后忽略 + 日志 warn

#### 2. CLI / 环境变量 / 命令

- CLI 新增 `--reasoning-effort <level>`（与现有 `--reasoning` 并列）
- 环境变量 `AEMEATH_REASONING_EFFORT=low`
- Slash 命令 `/effort [none|low|medium|high|xhigh]`（不带参数显示当前值，带参数切换）
- 配置优先级遵循 CLAUDE.md §1：CLI > env > 项目 config > 全局 config > 默认（medium）

#### 3. Provider 实现

**OpenAI 路径**（`aemeath-llm/src/providers/openai_compatible/non_stream.rs` + `stream.rs`）：
```rust
if reasoning_enabled && self.config.is_openai_reasoning_capable() {
    if let Some(effort) = &self.config.reasoning_effort {
        request_body["reasoning_effort"] = json!(effort);
    }
}
```

**Anthropic 路径**：未来可扩展为 `budget_tokens` 映射（Claude Opus/Sonnet 4.x thinking）：
- `low` → 1024
- `medium` → 4096
- `high` → 16384
- `xhigh` → 32768
- `none` → 不发 thinking 字段

**其他 provider**：忽略 `reasoning_effort`，沿用 on/off 开关。

#### 4. UI 反馈

- status bar 在 reasoning 启用时显示当前 effort 等级（如 `reasoning: high`）
- `/think` 命令保持兼容（仅 on/off），与 `/effort` 互补：
  - `/think off` → 等价于 `reasoning_effort = none`
  - `/think on` → 恢复上次 effort（默认 medium）

#### 5. 模型能力检测

在 `aemeath-core/src/provider.rs` 增加 `supports_reasoning_effort(model: &str) -> bool`：
- GPT-5 / GPT-5.2 / GPT-5.4 / GPT-5.5 / o1 / o3 / o3-mini → true
- 其他 OpenAI 模型 → false（GPT-4o 等不支持）
- 检测时按模型 id 前缀匹配，不匹配时静默忽略 `reasoning_effort` 字段

**测试场景**：
- 配置 `reasoning_effort=low` + GPT-5.5 → 请求体包含 `"reasoning_effort": "low"`
- 配置 `reasoning_effort=high` + GPT-4o → 请求体不包含该字段（不支持）
- 配置 `reasoning_effort=invalid` → 启动报错
- `/effort high` 后切换模型到 deepseek → effort 字段被忽略，日志 warn
- 不配置 `reasoning_effort` → 行为与现状一致（OpenAI 默认 medium）

**涉及路径**：
- `aemeath-core/src/config/models.rs`（新增 `reasoning_effort` 字段）
- `aemeath-core/src/provider.rs`（`supports_reasoning_effort` 能力检测）
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs` + `stream.rs` + `mod.rs`（构造请求体）
- `aemeath-cli/src/cli.rs`（`--reasoning-effort` flag）
- `aemeath-cli/src/main.rs`（env 变量读取 + 配置合并）
- 新增：`aemeath-core/src/command/commands/effort.rs`（`/effort` 命令）
- `aemeath-cli/src/tui/status_bar.rs`（effort 等级显示）

**关联**：与 `/think` 命令、`reasoning` 配置字段并存；后续可扩展到 Anthropic 的 `budget_tokens`。

#### 6. 待确认：GPT 思考内容当前不显示

**现象**：当前调用 OpenAI（含 GPT-5.x / o1 / o3 等具备 reasoning 能力的模型）时，TUI 中**看不到 thinking 内容**——只看到最终回答，没有任何 `> thinking…` 段或 reasoning 流式输出，与 Anthropic / DeepSeek 等 provider 行为不一致。

**需要确认的问题**：
1. **是否预期？**
   - OpenAI Chat Completions API 对 GPT-5.x 系列的 reasoning 内容**默认不返回**（出于费用 / 内容安全），仅在 Responses API 或显式开启 `reasoning.summary` 时才透出 reasoning summary
   - o1 / o3 系列 Chat Completions 同样不返回 thinking 文本，只返回 `usage.completion_tokens_details.reasoning_tokens`（计费用）
   - 因此"OpenAI 不显示 thinking"很可能是 **API 限制**而非 aemeath bug
2. **能否绕开？**
   - **方案 A**：切到 Responses API（`/v1/responses`），请求带 `reasoning: { "effort": "...", "summary": "auto" }`，响应里 `output[].type == "reasoning"` 段含 summary 文本——可作为 thinking 展示
   - **方案 B**：保留 Chat Completions，仅显示 reasoning_tokens 计数（"模型思考了 1234 tokens"），无文本内容
   - **方案 C**：维持现状，文档说明 OpenAI thinking 不可见

**排查行动**（与 #11 主体合并实施）：
1. 抓一次 GPT-5.5 + reasoning_effort=high 的实际响应体，确认 `choices[0].message` / `delta` 中是否真的没有 thinking 字段
2. 检查 `aemeath-llm/src/providers/openai_compatible/{stream,non_stream}.rs` 是否漏解析了 `reasoning_content` / `reasoning` / `thinking` 等字段
3. 对比 DeepSeek / Anthropic 路径如何把 thinking 推到 `UiEvent::Thinking`，确认 OpenAI 路径有无对应分支
4. 决策方案：是接 Responses API（方案 A）还是只显示 token 数（方案 B）

**测试场景**（确认后补充到 #11 主测试）：
- GPT-5.5 + effort=high → 抓包响应体，确认有/无 reasoning 段
- 切到 Responses API → reasoning summary 是否完整流式
- DeepSeek-R1（已知能显示 thinking）作为对照基线

**涉及路径**（额外）：
- `aemeath-llm/src/providers/openai_compatible/stream.rs`（流式 reasoning 解析）
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs`（非流式 reasoning 解析）
- 可能新增：Responses API 路径（`providers/openai_responses/`）

**开放问题**：
- 是否同期切到 Responses API？切换会影响所有 OpenAI 请求路径，是 #11 + 一个独立 feature 的工作量
- 若维持 Chat Completions，是否在 status bar 显示 "reasoning: 1234 tokens"（替代不可见的 thinking 文本）

**开放问题**：
- 是否需要把"按 effort 估算 budget_tokens"的 Anthropic 映射也一起做？还是分两期？
- `/effort` 切换是否在当前 turn 立即生效，还是下一个 turn 生效？（建议下一个 turn，避免请求中断）

---

### #12 Input Queue 双层循环优化

**目标**：让 LLM 在一个 user turn 内部（API call → tool calls → 下一次 API call → tool calls ...）的细粒度节点上**主动检查 input queue**，把用户排队的反馈尽早注入对话流，而不是等整个 agent loop 跑完才"看到"用户的新输入。让用户感受到"agent 听得见我"，而不是"agent 必须把这一摊事干完才理我"。

**背景**：
- Feature #7 已实现多消息 input queue（VecDeque），processing 期间用户可连续排队多条输入
- 当前消费时机是**外层 user-turn 循环**末尾——agent 完成所有 tool call、模型给出最终 stop_reason=EndTurn 后才 pop 一条 queue 进入下一轮
- 痛点：当 agent 进入长链路（连续 N 个 tool call、长 thinking、子 agent 嵌套）时，用户中途看到方向跑偏想纠正，目前必须等整轮结束才能让 agent 看到——体验上像"AI 自顾自跑"，用户反馈延迟极高
- Bug #21（粘贴入队语义）和 Feature #11（reasoning_effort）都是输入控制相关，本 feature 解决"何时让 agent 看到输入"

**设计**：

#### 1. 双层循环模型

```
outer loop: per user turn (现状)
  └─ inner loop: per agent step（API call + tool exec）
     ├─ 每次 inner 迭代开始前：检查 input queue
     ├─ 若 queue 非空：把队列内容作为 user message 注入 messages，跳过本轮原计划，继续 inner loop
     └─ 若 queue 为空：照常发起下一次 API call / 工具执行
```

inner loop 退出条件（沿用现状）：模型返回 `stop_reason = EndTurn` 且无 tool call。

#### 2. 检查点（粗到细）

按介入成本递增分级：

| 检查点 | 介入成本 | 说明 |
|--------|----------|------|
| **A. 每次 API call 前** | 低（必做） | 下一轮请求构造前 pop 全部 queue，作为 user message 拼到 messages 末尾。模型在下次回复时就能看到 |
| **B. tool call 批次完成后** | 低（必做） | 一批并行/顺序 tool call 跑完、准备发回 LLM 前，先 pop queue。最自然的"让 LLM 看到用户新指令"时机 |
| **C. tool call 之间（顺序）** | 中（可选） | 如果 tool call 改顺序执行（Bug #3 的修复方向），可在两个 tool call 之间检查；带"用户已发声"信号意味着后续 tool call 可能被取消 |
| **D. streaming 期间** | 高（不做） | 中断正在进行的 API call。语义复杂、provider 兼容性差，**不在本期范围** |

本期落地 **A + B**。C 留作后续扩展，需要 Bug #3 完成顺序执行后再做。

#### 3. 注入语义

用户排队消息进入 messages 时怎么标记？两种方案：

- **方案 1（普通 user message）**：直接 `Message::user(content)` 拼到末尾，模型自然继续对话
- **方案 2（带元数据的 system note）**：包成 `<user_interrupt>...</user_interrupt>` 或类似标签，提示模型"这是用户中途追加的反馈，请优先采纳"

推荐**方案 1 默认 + 方案 2 配置开关**。普通方案足够大部分场景；标签包裹在 agent 自主决策长链路被纠偏时有用。

#### 4. 取消进行中工作的策略

用户中途插话时，已经 in-flight 的 tool call 怎么办？

- **本期**：让进行中的 tool call **跑完**（不取消），跑完后注入用户消息，下一轮 API call 前模型自己决定要不要采纳
- **后期**（依赖 CancellationToken 基础设施）：选项化的"温柔取消"——给 in-flight tool 发取消信号，taken-effect 后注入用户消息

#### 5. 队列读取并发安全

- 当前 input queue 是 `VecDeque<String>` 包在 App 状态里，UI 线程 push、agent loop 主线程 pop
- 已有共享访问机制（具体待 grep 确认 `Arc<Mutex<...>>` / `tokio::sync::Mutex` / channel）
- 双层循环本期只是**多次调用同一个 pop 接口**，不改并发模型

#### 6. UI 反馈

- 用户在 processing 中输入并 Enter 后：input queue 区显示新条目（已有）
- inner loop 在 A/B 检查点 pop 到消息时：在 output area 注入一条 system 提示行 `[Injected from queue: "..."]`，让用户**看到**"我的反馈被吃进去了"，而不是默默并入下一轮 prompt
- 状态栏可临时高亮 1s 表示"queue 已消费"

#### 7. 配置

`config.json` 新增：
```json
{
  "input_queue": {
    "interrupt_mode": "between_calls",  // off | between_calls | between_tools
    "wrap_with_metadata": false          // 是否用 <user_interrupt> 标签包裹
  }
}
```

CLI 不暴露（属于体验设置，slash 命令 `/queue mode <...>` 切换）。

#### 8. 实施阶段

1. **Phase 1**（本期）：在 `agent_runner.rs` / `processing.rs` 的 inner loop A/B 检查点加 `pop_all_queued()` 调用 + UI 注入提示
2. **Phase 2**：增加 `<user_interrupt>` 包裹选项 + `/queue` slash 命令
3. **Phase 3**（依赖 Bug #3 顺序执行 + cancel 基础设施）：tool call 之间检查（C 检查点）、温柔取消进行中的 tool

**测试场景**：
- 用户 send 消息 → agent 进入 5 个 tool call 链 → 用户在第 2 个 tool 执行时排队 "stop, focus on X" → 期望：第 2 个 tool 跑完后，下一次 API call 前模型立即看到 "stop, focus on X" 并改变方向
- 用户连续排队 3 条 → 一次 pop 全部 → 拼成 3 条 user message 一起注入
- 队列在 inner loop 跑完都没消费过 → 退到 outer loop 时按原逻辑 pop（保持兼容）
- agent 在 ask_user 等待中（Bug #19 已修复）→ queue 不消费，等 ask_user 走完
- subagent 嵌套时：父 agent 的 queue 不应被子 agent 消费；子 agent 自己有独立 inbox（待决策，建议本期父子 agent 都不互通）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（agent 主循环 inner step）
- `aemeath-cli/src/tui/app/processing.rs`（user turn 顶层循环）
- `aemeath-cli/src/tui/app/mod.rs`（input queue 数据结构 + pop 接口）
- `aemeath-cli/src/tui/app/update.rs`（UI 注入提示）
- `aemeath-core/src/config/mod.rs`（`input_queue` 配置）
- 新增（Phase 2）：`aemeath-core/src/command/commands/queue.rs`

**关联**：
- Feature #7（input queue 基础实现，已完成）
- Bug #21（粘贴入队语义）— 必须先确保入队来源干净
- Bug #3（tool call 流式 + 顺序执行）— Phase 3 的 C 检查点依赖
- Bug #19（ask_user 等待态独占 input，已修复）— queue 消费时需绕开 ask_user 状态

**开放问题**：
- 子 agent 是否共享父 agent 的 input queue？默认不共享，但 deeply nested agent 时父用户反馈如何透传？
- 标签包裹 `<user_interrupt>` 是否 model-agnostic？某些模型可能把它当 XML 字面量解析
- 用户排队"取消当前 tool call"语义如何表达？需要一个特殊关键字 / 命令前缀（例如 `/cancel`）还是 LLM 自行从语义判断？

---

### #13 Task list 显示在 spinner 下方

**目标**：把 TUI 临时区域中 task status lines（`✓ / ■ / □ + subject` 任务列表）从 spinner 上方挪到 spinner 下方，让 spinner 紧贴正在流动的输出，而 task list 作为更稳定的"任务面板"位于底部，与 input area 视觉相邻。

**当前现象**：

Bug #24 修复后，临时区域渲染顺序为：

```
queued messages
task status lines      ← task list 当前在 spinner 之上
spinner                ← spinner 紧贴 input area
─────────────
input area
```

实际效果：
- spinner 紧贴 input area，看上去"思考动效"贴住了输入框，与正在生成的内容（output area）距离较远
- task list 夹在 queued messages 与 spinner 之间，每次任务条数变化都会把 spinner 上下推动一行，spinner 像在跳
- 用户感知：task list 是相对稳定的状态，应该"沉底"；spinner 是动效，应该贴近正在生成的内容

**预期行为**：

调整为：

```
queued messages
spinner                ← spinner 紧贴 output 流
task status lines      ← task list 沉到底部，挨着 input area
─────────────
input area
```

理由：
1. spinner 表达"agent 正在工作"，紧贴 output 区域更符合视觉因果（"上面的内容由这个 spinner 推动出来"）
2. task list 是一个相对稳定的"任务面板"，每条任务进入/完成才变化，沉到 input area 上方变成视觉锚点
3. spinner 不再被 task list 条目数变化挤上挤下，动效更稳定

**影响 / 与 Bug #24 的关系**：

- Bug #24 之前的顺序就是 `queued → spinner → task`，但当时因为最终裁剪从底部保留导致 spinner 被挤出，所以才改成 `queued → task → spinner`
- 本期需要把"spinner 永远可见"的保证迁移到新顺序下：当临时区域行数超出可见区域必须裁剪时，**优先裁掉 task status lines 的中部**（保留头几条 + 省略号），而不是裁 spinner
- 也就是说 spinner 行的优先级要高于 task status lines 的"完整列出"

**实现方向**：

1. `aemeath-cli/src/tui/output_area/mod.rs` 调整临时行追加顺序：
   - 改回 `queued messages → spinner → task status lines`
2. 临时区域裁剪策略改为：
   - 必保：spinner 行
   - 次优：queued messages（已有处理）
   - 可截断：task status lines（超长时显示前 N 条 + `… +M more` 行）
3. 验证 Bug #24 描述的场景不退化：
   - 大量 task 行时 spinner 仍可见
   - tool call 切换、queued message 增减时 spinner 不闪烁

**测试场景**：
- 单个 in_progress task → task 行紧贴 input area 上方，spinner 在 task 行之上
- 5+ 条 task（混合 done / in_progress / pending）→ task 列表沉底，spinner 仍位于 task 之上、output 之下
- 临时区域空间不够 → spinner 必显示，task 列表显示前 N 条 + 折叠提示
- 没有 task 时 → spinner 直接挨着 input area，行为退化为 spinner 在底部
- queued messages 同时存在 → 顺序为 queued → spinner → tasks，三段都可见

**涉及路径**：
- `aemeath-cli/src/tui/output_area/mod.rs`（临时区域追加顺序、裁剪策略）
- `aemeath-cli/src/tui/output_area/spinner.rs`（不变，但需确认 spinner 行高仍为 1）
- `aemeath-cli/src/tui/app/render.rs`（reserved height 计算需对应新顺序）

**关联**：
- Bug #24（spinner 偶尔消失，已修复）—— 顺序调整不能让 spinner 重新被挤出，需用裁剪策略保证
- Bug #25（/clear 未清空 status line）—— 顺序调整不影响 reset 路径，但完成时一并验证

**开放问题**：
- task list 折叠阈值取多少合适？建议默认 5 行，超过显示前 4 + `… +N more`
- 是否需要配置项让用户切换"task 在上 / 在下"？倾向不开口子，统一一种布局

---

### #15 通过 max_tokens 限制 LLM thinking 长度

**目标**：让用户可以为 thinking 阶段设置一个**最大 token 上限**，避免模型在简单问题上"过度思考"——例如打招呼也跑出几千 token 的内心独白，浪费 token 费用与等待时间。统一映射到各 provider 的 thinking budget 字段，让一个配置控制所有 reasoning provider。

**背景**：

不同 provider 控制 thinking 长度的方式不同：

| Provider | 字段 | 取值 |
|----------|------|------|
| Anthropic | `thinking.budget_tokens` | 整数（>=1024，模型上限通常 32K~64K） |
| OpenAI GPT-5.x | `reasoning_effort` | 离散等级 `none/low/medium/high/xhigh`（无显式 token 上限） |
| OpenAI Responses API | `reasoning.max_tokens` | 整数（部分模型支持） |
| DeepSeek | `thinking.type` + 隐式上限 | 仅 on/off，无显式上限 |
| GLM / Qwen | `enable_thinking` | 仅 on/off |

aemeath 当前在 `aemeath-llm/src/providers/` 各 provider 里只有 reasoning on/off 控制，没有统一的"thinking 上限"语义。Feature #11 已规划 OpenAI 的 `reasoning_effort` 等级控制，但没覆盖 Anthropic / Responses API 的精确 token 控制。

**设计**：

#### 1. 配置字段

模型级新增 `thinking_max_tokens: Option<u32>`：

```json
{
  "name": "claude-opus-4-7",
  "provider": "anthropic",
  "model": "claude-opus-4-7",
  "reasoning": true,
  "thinking_max_tokens": 4096
}
```

- 取值范围：1024 ~ 模型上限（Anthropic 通常 32K，OpenAI Responses API 模型按文档）
- 不配置时使用 provider 默认（Anthropic 不传 budget_tokens 走默认；OpenAI 走 effort=medium）
- 与 `reasoning_effort` 互斥优先级：精确数字优先；同时配置时数字胜出 + 日志 warn

#### 2. CLI / 环境变量 / 命令

- CLI：`--thinking-max-tokens <N>`
- 环境变量：`AEMEATH_THINKING_MAX_TOKENS=4096`
- Slash 命令：`/budget [N|off]`（不带参数显示当前值；`off` 等价 reasoning 关闭；数字直接设上限）
- 配置优先级遵循 CLAUDE.md §1：CLI > env > 项目 config > 全局 config > 默认

#### 3. Provider 映射

**Anthropic**（`aemeath-llm/src/providers/anthropic/`）：
```rust
if reasoning_enabled {
    let budget = config.thinking_max_tokens.unwrap_or(default_budget);
    request_body["thinking"] = json!({
        "type": "enabled",
        "budget_tokens": budget
    });
}
```

**OpenAI Chat Completions（GPT-5.x / o-series）**：
- Chat Completions 不支持精确 token 上限，需要把 `thinking_max_tokens` **反向映射**为 effort 等级：
  - `<=1024` → `low`
  - `<=4096` → `medium`
  - `<=16384` → `high`
  - `>16384` → `xhigh`
  - `0` → `none`
- 同时在 status bar / 日志里说明"OpenAI 不支持精确数字，已映射为 effort=high"

**OpenAI Responses API**（如果走 #11 §6 决策切到 Responses API）：
- `reasoning.max_output_tokens`（按当时 API 字段名为准）直接传入
- 优先于 effort 等级

**DeepSeek / GLM / Qwen**：
- 不支持精确控制，沿用 on/off
- 配了 `thinking_max_tokens` 时日志 warn"该 provider 不支持，已忽略"

#### 4. UI 反馈

- status bar 在 reasoning 启用时显示 `thinking: ≤4096`（数字配置时）或 `thinking: high`（仅 effort 时）
- 实际响应回来后，若 provider 上报 `usage.completion_tokens_details.reasoning_tokens`，可显示"thinking used: 1234 / max 4096"作为审计

#### 5. 默认建议值

- 不配置 `thinking_max_tokens` 时各 provider 默认：
  - Anthropic：不发 budget_tokens（让 API 自己默认）
  - OpenAI：effort=medium
  - 其他：on/off

文档中给一份"推荐预设":
- 简单对话：1024
- 编程协助：4096（默认推荐）
- 深度推理 / 长链路 debug：16384
- 极限：32768（注意成本）

#### 6. 与 #11 的协作

本 feature 与 Feature #11 强相关，建议**合并实施**或在 #11 基础上做：
- #11 解决"OpenAI effort 等级配置 + 模型能力检测"
- #15 解决"统一 max_tokens 数字 + 映射到各 provider"
- 实施顺序：先 #11（拿到 effort 字段管线）→ 再 #15（在管线上加 max_tokens 反向映射）

**测试场景**：
- Anthropic claude-opus-4-7 + `thinking_max_tokens=2048` → 请求 `thinking.budget_tokens=2048`
- OpenAI GPT-5.5 + `thinking_max_tokens=2048` → 请求 `reasoning_effort=medium`（反向映射）+ 日志说明
- OpenAI GPT-4o + `thinking_max_tokens=4096` → 不支持 reasoning，整个字段忽略
- 同时配置 `reasoning_effort=high` + `thinking_max_tokens=512` → 数字胜出 → 映射为 `effort=low` + warn
- DeepSeek-R1 + `thinking_max_tokens=4096` → 字段忽略 + warn，仅按 on/off 启用
- 默认值（不配置）→ 行为与现状一致

**涉及路径**：
- `aemeath-core/src/config/models.rs`（新增 `thinking_max_tokens` 字段 + 校验）
- `aemeath-core/src/provider.rs`（`map_thinking_budget_to_effort` 反向映射函数）
- `aemeath-llm/src/providers/anthropic/`（构造 `thinking.budget_tokens`）
- `aemeath-llm/src/providers/openai_compatible/`（反向映射 effort）
- `aemeath-cli/src/cli.rs`（`--thinking-max-tokens` flag）
- `aemeath-cli/src/main.rs`（env 变量读取 + 配置合并）
- 新增：`aemeath-core/src/command/commands/budget.rs`（`/budget` 命令）
- `aemeath-cli/src/tui/status_bar.rs`（thinking 上限显示）

**关联**：
- Feature #11（reasoning_effort 配置）—— 强依赖，#11 提供 OpenAI effort 字段管线
- Feature #11 §6（GPT thinking 不显示）—— 若切到 Responses API，可直接用 `reasoning.max_output_tokens`

**开放问题**：
- 反向映射阈值（1024 / 4096 / 16384）是否合理？需测几款 OpenAI reasoning 模型实际 thinking_tokens 分布
- 是否需要"软上限"——超过 max_tokens 时模型自行收尾而不是被硬截？Anthropic 的 budget_tokens 是软上限（提示模型尽量不超），OpenAI effort 是隐式控制，行为不一致
- `/budget 0` 是否应该等价 `/think off`？倾向是，避免两条命令各管一半
- 是否同期做 Responses API 切换？建议解耦，本期先用 effort 反向映射，Responses API 切换走独立 feature

---
**开始日期**：2026-04-27
