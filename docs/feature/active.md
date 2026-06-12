# 活动中 Feature

> 排序规范：表格行和详情区块均按 ID 升序排列。

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 8 | Memory 系统 | - | 已完成 | 未确认 | MVP 已落地：MemoryConfig/Store/命令/Tool/session reminder；system prompt 只保留 Memory tool 提示 |
| 9 | 反思系统 | - | 已完成 | 未确认 | 已接入 /reflect、auto_apply、N 轮自动触发；Compact 前提取 memory 建议 |
| 28 | MCP 系统完善 | 高 | 🔧 未完成 | 未确认 | P0+P1 已完成；SSE 传输有可靠性问题，MCP 加载暂时禁用待修复 |
| 34 | Anthropic Claude 原生 Provider | 高 | ✅ 已完成 | 未确认 | 原生 Anthropic Messages API 适配（流式/非流式/thinking/重试/tool use） |
| 42 | 权限管控系统 | 高 | 设计中 | 未确认 | 交互式外部授权 + 统一 PermissionEngine + audit/policy 域；详见 [spec](specs/042-permission-control-system.md) |
| 49 | AskUserQuestion 增加 All/Chat 选项 | 中 | ✅ 已完成 | 未确认 | 已实现 All/Chat 内建选项、chat about this 自由输入态、选项双行渲染 |
| 52 | Tool 描述英文化 | 中 | 未开始 | 未确认 | 将 EnterWorktree/ExitWorktree 两个 tool 的中文描述统一为英文 |
| 68 | 项目指令搜索增强：全局 fallback + 向上 5 级目录搜索 | 中 | 修复中 | 未确认 | 全局 fallback `~/.claude/CLAUDE.md`；项目指令向上 5 级搜索，不向下递归 |
| 69 | TUI Hook 消息类型化与 system-reminder 展示脱壳 | 中 | 活动中 | 未确认 | Hook 消息类型化（HookNotice），system-reminder TUI 展示脱壳 |
| 77 | diff removed 行不语法高亮，只显示纯红色 | 低 | 待确认 | 未确认 | removed 行改为纯 `DIFF_REMOVE_FG` 红色，不调用语法高亮 |
| 78 | CLI 增加 `-q` 无 TUI 模式和 `-v` 日志输出到 stderr 模式 | 中 | 活动中 | 未确认 | `-q` 跳过 TUI 直接 REPL，`-v` 日志输出到 stderr |
| 79 | 日志模块整理与 hook 可观测性增强 | 中 | ✅ 已完成 | 待用户确认 | 移除废弃 `module_levels`，统一全局过滤；补充 hook 初始化/匹配/分发日志 |
| 80 | Agent context 所有权重构（project 拥有 WorkspaceState） | 高 | ✅ 已完成 | 未确认 | workspace 可变状态收敛为 project 单一 WorkspaceState，feature 经能力 trait 访问，子 agent 隔离，git 抽 outbound port |
| 81 | TUI assistant 文本与 spinner phase 视觉调整 | 低 | 待确认 | 未确认 | spinner phase 移除 emoji；assistant 文本前增加白色圆点 gutter |
| 82 | Provider/Config 设计债收口 | 中 | 未开始 | 未确认 | 统一 API key 解析与 pool 配置路径、unknown driver 显式报错、默认值收口、spec 同步 |
| 83 | Tool result 统一结构化 JSON | 高 | 实现中 | 未确认 | 所有 tool 返回统一 JSON payload，LLM 保留完整结构，TUI 按工具选择字段展示 |
| 84 | Stop hook 命令显示短路径 | 低 | ✅ 已完成 | 未确认 | TUI 显示 hook stop 命令时只展示脚本名，避免完整项目路径过长 |
| 85 | Read/TaskList tool call 单行摘要展示 | 中 | 待确认 | 未确认 | Read 显示读取范围与总行数；TaskListCreate/TaskUpdate 单行摘要；失败原因仅在 result 中展开 |
| 86 | 粘贴长文本自动折叠为 `[Copied Text n]` | 中 | 待确认 | 未确认 | input area / input queue 中超过两行的粘贴文本折叠显示为 `[Copied Text n]`，TUI 原样渲染 |

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

### #9 反思系统

**状态**：已完成（待确认）

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思，将有价值的经验写入 Memory 系统（#8）。

#### 完成度评估（2026-05-19）

**已实现**：

| 组件 | 位置 |
|------|------|
| `ReflectionEngine`（解析 JSON、格式化输出） | `reflection/` |
| 完整 LLM reflection runner（Prompt 构建、调用 provider、解析结果） | `reflection/runner.rs` |
| Prompt 模板（偏差检测 + 建议记忆 + 过时记忆） | `reflection/prompt.rs` |
| 主循环 N 轮自动触发（`reflection.interval_turns`，0 禁用） | `chat/looping/reflection.rs` |
| Compact 前基于 early messages 提取 memory 建议，默认不自动写入；PostCompact 不再作为目标 | `chat/looping/compact.rs`、`compact/summary.rs` |
| SDK/TUI `/reflect` 调用完整 LLM reflection runner | `packages/sdk/src/tui.rs`、`tui/app/slash/reflection.rs` |
| `/reflect apply` 将 pending 建议写入 MemoryStore | `tui/app/slash/reflection.rs` |
| `auto_apply_suggestions` 自动应用 suggested memories 与 outdated markers | `reflection/apply.rs`、`tui/app/update/ui_event.rs` |
| TUI 自动 reflection 完整展示结果，并保留 pending 建议 | `tui/app/update/ui_event.rs` |
| 手动 `/reflect` 刷新 pending，支持后续 `/reflect apply` | `tui/app/slash/reflection.rs` |

**暂缓/未实现**：连续工具失败、SessionEnd/SubagentStop、独立 reflection model、`ReflectionGenerated` hook、stats/history 持久化。

**说明**：PostCompact 不再作为目标；当前已改为 Compact 前基于即将被压缩的 early messages 提取 memory 建议。

**涉及路径**：`reflection/`、`tui/app/slash/reflection.rs`、`tui/app/update/ui_event.rs`

---

### #28 MCP 系统完善

**目标**：完善 MCP 系统的配置、连接管理、工具发现注册和 CLI 操作路径，覆盖本轮 P0+P1 基础能力；P2 不在本轮范围，待后续单独规划。

#### 本轮已完成的 P0+P1 改动

- `McpServerConfig` 支持 stdio 可用配置；SSE/Streamable HTTP 仅完成配置解析与 URL 安全校验，传输实现仍为占位存根；header 脱敏已完成。
- McpConnectionManager 接入启动加载路径，统一管理连接/tool 发现/注册。
- ToolRegistry 注销与查询接口。
- Manager tool diff、snapshot refresh、`health_check_once` API 和状态转换；未暗示后台定时 health check loop 已启动。
- `/mcp` 独立命令模块已落地，支持 list/tools/restart/add/remove 的解析与预览输出；实际运行时操作待 runtime bridge 接入。
- MCP tool result 默认 1MB 响应大小限制。

#### 已知限制

- SSE 传输存在可靠性问题：z.ai 等远程 MCP server 的 SSE response 在 tools/list 时经常出现超时或不完整（2890 bytes 后 SSE stream 停止发送，缺少 `\n\n` 终结符）。已尝试 POST body 消费、独立 client、incomplete event fallback 等方案，均未根本解决。
- **MCP 加载已从 TUI 启动流程中禁用**（`run_orchestration.rs` 中 `spawn_mcp_connect` 调用已注释），避免启动时因 MCP 连接超时阻塞。
- `/mcp` restart/add/remove/list/tools 暂未接入 runtime manager bridge，仅返回状态/预览提示。
- Streamable HTTP 传输未实现。
- 健康检查已有单次 API，后台定时 loop 尚未接入。

#### 待修复方向

- SSE stream 可靠性：考虑改用 Streamable HTTP 传输（单 POST 请求返回完整 JSON-RPC response，不依赖 SSE event stream）。
- 或者在 SSE transport 中增加更健壮的重试机制和超时策略。
- 需要测试更多 MCP SSE server 以确认是 z.ai 特定问题还是通用问题。

#### 关联

- Feature #28 MCP P0+P1 实施任务 1-9 已完成并 review 通过
- 用户确认后再归档；确认前保留在 active.md

---

### #34 Anthropic Claude 原生 Provider

**目标**：作为独立 LLM provider，支持直接调用 Anthropic Claude Messages API，与现有 OpenAI/OpenRouter/DeepSeek 等并列。

**已完成的改动**：

1. **ApiDriverKind::Anthropic**：`aemeath-core/src/provider.rs` 新增 `Anthropic` 变体，且为默认 provider。
2. **AnthropicProvider**（`aemeath-llm/src/providers/anthropic/`）：
    - 原生 Messages API 调用（`POST /v1/messages`）
    - 流式响应解析 + 非流式 fallback（streaming 失败且无 partial output 时自动降级）
    - Thinking budget（extended thinking）：`thinking_max_tokens > 0` 时自动开启 `thinking.type=enabled`
    - 指数退避重试（最多 10 次），支持 429 / 5xx 自动重试
    - 用户取消（CancellationToken）全路径支持
    - Prompt caching beta header（`anthropic-beta: prompt-caching-2024-07-31`）
3. **消息转换**（`anthropic/message_conversion.rs`）：TrackingHandler 检测 partial output 避免非流式 fallback 重复输出。
4. **请求构造**（`types.rs::CreateMessageRequest`）：thinking budget 序列化、tool schema 嵌入。
5. **池化集成**（`pool.rs`）：Anthropic driver 走独立 provider 分支，不经过 OpenAICompatible wrapper。

**涉及路径**：
- `aemeath-core/src/provider.rs`（ApiDriverKind::Anthropic）
- `aemeath-llm/src/providers/anthropic/mod.rs`（AnthropicProvider 实现）
- `aemeath-llm/src/providers/anthropic/message_conversion.rs`（非流式 fallback + TrackingHandler）
- `aemeath-llm/src/client.rs`（with_provider 匹配 Anthropic 分支）
- `aemeath-llm/src/pool.rs`（Anthropic 跳过 OpenAIProviderConfig 构建）
- `aemeath-llm/src/types.rs`（CreateMessageRequest thinking 序列化）

**测试**：`anthropic_request_serializes_thinking_budget`、`anthropic_request_omits_thinking_when_budget_zero` + `provider.rs` 6 个单元测试。

---

### #42 权限管控系统

**状态**：设计中

**目标**：AllowAll 模式下 Glob/Grep 访问 workspace 外路径仍被拦截。升级为完整权限管控：交互式授权 + 统一 `PermissionEngine`（action/resource/risk → Allow/Ask/Deny）；权限模式 AskMe/Auto/Plan/AllowAll。详见 [spec](specs/042-permission-control-system.md)。

**范围**：
1. `PermissionEngine` + 统一权限模型
2. 外部路径授权 → TUI 交互式选择
3. `ToolContext` 保存 session 级授权 scope
4. Read/Glob/Grep 先接入；Edit/Write 后续接入
5. AllowAll 下允许 workspace 内外读写，仅审计

**涉及路径**：`permission.rs`、`tool.rs`、`path_security.rs`、`file_read/glob/grep/file_edit/file_write.rs`、TUI `permissions.rs`/`tools.rs`

---

### #49 AskUserQuestion 增强——title+description 选项、智能 All/None、Type something 输入框

**状态**：已完成（c98c26c），待确认

**实现**：
1. **OptionItem 类型**：sdk 层新增 `OptionItem { title, description }`，向后兼容纯字符串
2. **智能内建选项**：≥2 LLM 选项时追加 All/None/Type something；1 个追加 None/Type something；0 个纯自由输入
3. **Type something 输入框**：选中后进入行内编辑态，Up 返回选项列表，Enter 提交，Esc 取消
4. **选项渲染**：title 加粗 + description 灰色缩进双行布局
5. **工具 schema 更新**：options 支持 `oneOf(string, object { title, description })`

**变更范围**：sdk, runtime, tools, TUI 全链路（18 files, +404 -96）

**涉及路径**：AskUserQuestion options 渲染、输入态切换、回答构造、文案常量

**验收标准**：选项末尾稳定出现 All/Chat；All 回传结构化选项集合；Chat 进入自由输入态；内建选项不与 LLM option 重名冲突

---

### #52 Tool 描述英文化：所有 tool 给 LLM 的 description 统一为英文

**状态**：未开始

**背景**：当前所有内置 tool 中，大部分 `description()` 已是英文，仅 `EnterWorktree` 和 `ExitWorktree` 两个 tool 的 description 和 input_schema 参数描述仍为中文。LLM 对英文描述的语义理解更精确，统一为英文有助于减少工具调用错误。

**目标**：将所有内置 tool 给 LLM 的 description 和 input_schema 参数描述统一为英文。

**范围**：
1. 内置 tool：`EnterWorktree`、`ExitWorktree` 的 description 和 input_schema 参数描述改为英文。
2. 审查所有内置 tool 的 `input_schema` 参数 `description` 字段，确认无中文残留。
3. MCP tool 的 description 由 MCP server 返回，不在本 feature 范围内（透传不改动）。

**涉及文件**：
- `agent/tools/src/worktree.rs`：`EnterWorktree`、`ExitWorktree` 的 `fn description()` 和 `fn input_schema()` 实现
- 可能涉及：`agent/tools/src/` 下其他 tool 的 `input_schema` 参数描述审查

**验收标准**：
1. `EnterWorktree` 和 `ExitWorktree` 的 `description()` 返回纯英文描述。
2. `EnterWorktree` 和 `ExitWorktree` 的 `input_schema()` 中所有参数 `description` 字段为英文。
3. 全量审查通过：29 个内置 tool 的 description 和 input_schema 参数描述均为英文。
4. 编译通过（`cargo build -p aemeath-tools`）。

### #68 项目指令搜索增强：全局 fallback ~/.claude/CLAUDE.md + 向上/向下 5 级目录探索

**状态**：已完成（4f2e5e1 + 2aecab7），待确认

**背景**：全局指令只读 `~/.agents/AGENTS.md`，不兼容 Claude Code 的 `~/.claude/CLAUDE.md`；项目指令只在 cwd 目录搜索，无法发现父目录或子目录中的指令文件。

**目标**：
1. 全局指令优先 `~/.agents/AGENTS.md`，不存在时 fallback `~/.claude/CLAUDE.md`
2. 项目指令从 cwd 向上 5 级祖先目录 + 向下 5 级子目录搜索，每层级 `CLAUDE.md` 优先于 `AGENTS.md`
3. 找到第一个存在的文件即停止（保持 break 语义）

**涉及路径**：
- `agent/shared/src/config/paths.rs`：新增 `INSTRUCTION_SEARCH_DEPTH` 常量
- `agent/features/runtime/src/business/prompt/build/prompt_build.rs`：`load_agents_md` 全局双路径 fallback + `project_instruction_walk` / `push_instruction_paths_for_dir` 函数
- `agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`：3 个 walk 测试

### #69 TUI Hook 消息类型化与 system-reminder 展示脱壳

**状态**：待确认

**背景**：Stop hook 阻止结束时，反馈既需要作为 `<system-reminder>` 注入下一轮 LLM 上下文，也需要在 TUI 中提示用户。但当前用户可见提示沿用普通 `SystemMessage`/`SystemNotice` 展示，视觉上接近用户输入；部分场景还可能把 `<system-reminder>` 标签原样展示，造成用户误以为系统内部标签进入了可见对话内容。后续 StopFailure 或其他 hook 也可能产生不同语义的用户可见消息，继续复用普通 SystemMessage 会让样式、文案和脱壳规则分散。

**目标**：
1. TUI 展示层对 `<system-reminder>...</system-reminder>` 包装做统一脱壳，只展示内部的人类可读内容。
2. 新增 Hook 类用户可见消息，避免 Hook 反馈混用普通 SystemNotice；Hook 消息应能承载来源事件或语义类型，便于 Stop、StopFailure 和未来 Hook 使用不同文案/样式。
3. 保留 LLM 上下文注入中的 `<system-reminder>` 语义，不改变模型可见系统提醒协议；脱壳仅作用于 TUI 可见展示。
4. Hook 阻止类消息在视觉上应与用户输入明显区分，优先使用 warning/error 语义色或明确前缀。

**建议实现方向**：
1. 已在 SDK/runtime 事件层引入统一 `HookEvent`/`HookEventStatus`，用 `Running`、`Succeeded`、`Blocked`、`Failed` 表达 hook 生命周期和结果。
2. 已移除旧 `HookStart`、`HookEnd`、`StopFailureHook` 事件路径；所有 hook 执行统一发送 `HookEvent`。
3. 已在 TUI adapter/model 层新增 `ConversationBlock::HookNotice` 与 `OutputBlockKind::HookNotice`，由 TUI 根据 `HookEvent` 派生 blocked/failed notice 文案和 warning/error 样式。
4. 已抽出单一 helper 剥离完整包裹的 `<system-reminder>` 标签，并应用到 TUI 可见 SystemNotice/HookNotice 展示。
5. Stop hook blocked 不再发送普通 `SystemMessage` 作为用户提示，但仍保留返回给 loop 的 feedback，用于继续注入 LLM 上下文。

**验收标准**：
1. 当用户可见消息文本为完整 `<system-reminder>...</system-reminder>` 包装时，TUI 输出区不显示开始/结束标签。
2. Hook 反馈以 Hook notice 类型进入 conversation/view model，不再只依赖普通 SystemNotice。
3. Stop hook blocked 提示仍会显示命令和失败详情；长输出写入文件路径等信息不丢失。
4. LLM messages 中用于继续对话的 `<system-reminder>` 包装保持不变。
5. 单元测试覆盖：脱壳正常路径、无标签边界、标签不完整/嵌入普通文本时不误删，以及 Hook notice 的样式/类型映射。

**明确不做**：
1. 不重做所有 SystemNotice 的视觉设计；本 feature 只处理 Hook 类消息和 system-reminder 脱壳。
2. 不改变 Hook 执行协议、JSON schema 或阻止逻辑。
3. 不把所有 LLM system reminder 从消息历史中移除；仅区分模型上下文与 TUI 展示。

**验证**：
- `cargo check`（baseline）
- `cargo check -p runtime -p sdk`
- `cargo check -p cli`
- `cargo test -p cli hook_notice --bins`
- `cargo test -p cli system_reminder --bins`
- `cargo test -p runtime stop_hook --lib`

**涉及路径（预计）**：
- `agent/features/runtime/src/business/chat/looping/finalize.rs`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- `packages/sdk/src/*`（如需新增 ChatEvent 类型）
- `apps/cli/src/tui/effect/session/processing.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/*`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/render/output/blocks/diagnostic.rs` 或新增 Hook notice renderer

### #77 diff removed 行不语法高亮，只显示纯红色

**状态**：待确认

**背景**：diff removed/delete 行当前会把正文交给 syntect 语法高亮，导致删除行中出现关键字、标点等语法色。Feature #77 要求删除语义行保持单一删除色，避免红色删除语义被语法色冲淡。

**设计**：
1. 采用 deleted/removed 专用 helper，而不是给通用高亮 helper 增加开关。
2. unified diff 的 `DiffLineKind::Removed` 只保留 `-` prefix 与纯 `DIFF_REMOVE_FG` 正文 span。
3. 普通 diff 的 `build_delete_line` 不再接收 `syntax_ref`，避免误用语法高亮。
4. added/context 行继续沿用现有 syntect 高亮逻辑。

**验证**：
- `cargo test -p cli unified_diff -- --nocapture`
- `cargo test -p cli diff -- --nocapture`

**涉及路径**：
- `apps/cli/src/tui/render/output/primitives/unified_diff.rs`
- `apps/cli/src/tui/render/output/diff.rs`

### #79 日志模块整理与 hook 可观测性增强

**状态**：设计中（待用户确认）

**背景**：
1. `module_levels` 已从 `LoggingConfig` 中移除，但 `docs/feature/archived/040-claude-compatible-agents-config.md` 记录了该决策；代码与配置层面已无残留。
2. `aemeath.log` 当前通过 `env_logger::Builder` 接收全部 `log::*` 宏输出，所有模块的诊断日志混在一个文件中，TUI 渲染日志与 runtime 业务日志相互淹没，职责不清。
3. Hook 可观测性不足：runner.rs 中常规流水日志为 `info` 级别，在默认 `warn` 级别下不可见。
4. 日志格式为纯文本，不利于机器解析和后续分析。
5. 日志上下文仅含 `session` + `turn`，缺少 `chat_id` 和 `model`。

**设计决策**：

#### A. 统一日志入口：`UnifiedLogger`（路径 C）

**所有日志走一个 `log::Log` 入口 + 按 `record.target()` 前缀路由**。`JsonLogger` 与 `RoutingLogger` 合并为 `UnifiedLogger`，消除双管线：

| target 前缀 | 路由目标 | 写入格式 | 类别 |
|-------------|----------|----------|------|
| `cli::*` | `tui.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| `hook::*` | `hook.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| `audit::input` | `input.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| `audit::output` | `output.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| `audit::tool` | `tool.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| 其他 | `aemeath.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| panic | `panic.log` | 纯文本 + backtrace | 崩溃日志 |

> **统一为 JSON Lines**：诊断与审计均保证"**一行一个 JSON**"。诊断是固定 schema（`ts/session/chat/turn/model/level/target/msg`），审计是 `serde_json::Value::to_string()`（**compact，不带缩进**）单行序列化。`grep` / `jq` 可同时消费 `*.log`。

**`UnifiedLogger` 暴露两层 API**：

1. **`impl log::Log for UnifiedLogger`**（诊断入口）：接受 `&Record`，按 `record.target()` 前缀路由到对应文件 sink。调用方仍是 `log::info!(target: "cli::render", "msg")` 等宏，零侵入。
2. **结构化 sink 方法**（审计入口）：
   ```rust
   UnifiedLogger::log_input(json: serde_json::Value)
   UnifiedLogger::log_output(json: serde_json::Value)
   UnifiedLogger::log_tool(json: serde_json::Value)
   ```
   审计数据不经过 `log::*` 宏，直接以 `serde_json::Value` 形态写入对应审计文件，保留结构。

**`enabled()` 行为**：
- 诊断日志：`UnifiedLogger::enabled(record)` 委托 `env_logger::Logger::enabled()`，按 `RUST_LOG` + `config.level` 过滤
- 审计日志：`log_input/output/tool()` 内部先查 `role_logs_enabled && env_logger::enabled(...)`，确保审计开关与日志级别双控制

**为什么这是真正"统一"**：
- 调用方只面对一个 logger（`log::Log` trait）
- 审计与诊断共享 `enabled()` 过滤逻辑，但写入路径分 sink 保持各自数据形态
- 避免了路径 B（`log::info!(target: "audit::input", "{:#?}", json)`）的"序列化→字符串→反序列化"退化

#### B. 统一 JSON Lines 格式（诊断 + 审计）

所有日志文件均为 **JSON Lines**：每行一个完整 JSON 对象，行间无依赖。

**诊断日志**（`aemeath.log` / `tui.log` / `hook.log`）固定 schema（8 字段）：
```json
{"ts":"2026-06-11T14:30:00+08:00","session":"abc123","chat":"session-abc123-001","turn":3,"model":"deepseek/deepseek-chat","level":"INFO","target":"runtime::business::chat","msg":"chat started"}
```

字段：`ts` / `session` / `chat` / `turn` / `model` / `level` / `target` / `msg`

**审计日志**（`input.log` / `output.log` / `tool.log`）形态为**单行 `serde_json::Value`**：
```json
{"ts":"2026-06-11T14:30:00+08:00","session":"abc123","chat":"session-abc123-001","turn":3,"model":"...","messages":[{"role":"user","content":"..."}],"tools":[...]}
```

- 序列化：`serde_json::to_string(&value)`（**compact，不带缩进**），保证单行
- 字段：调用方提供的 `serde_json::Value` 顶层加 `ts/session/chat/turn/model` 上下文后整体序列化
- 嵌套结构可任意深度，consumer 用 `jq '.messages[0].content'` 即可下钻

**消费者一致性**：`grep -E '^\{' *.log | jq` 同时处理诊断与审计；`jq -c` 强制 compact 输出便于管道传递。

#### C. 全局日志上下文注入

| 变量 | 类型 | setter 位置 | 说明 |
|------|------|------------|------|
| `SESSION_ID` | `OnceLock<String>` | 会话启动 | 已有 |
| `CURRENT_CHAT_ID` | `RwLock<String>` | `loop_runner.rs` 每 chat 开始 | **新增**（chat 生命周期内变化） |
| `CURRENT_TURN` | `AtomicUsize` | 每 turn 开始 | 已有 |
| `CURRENT_MODEL` | `RwLock<String>` | `setup.rs` model 解析后 | **新增**（模型可能切换） |

#### D. Hook 日志降级

`hook/runner.rs` 中 5 处常规流水 `log::info!` → `log::debug!`：
- `hook match`
- `hook start`
- `hook env stdout/stderr`
- `hook end`

错误路径（spawn failed / wait failed / timeout / non-zero exit）保持 `warn`。

#### E. 职责边界规范

`aemeath.log` / `tui.log` / `hook.log` 禁止打印 messages / content blocks / tool results 等 LLM 交互数据；此类数据**必须**走 `UnifiedLogger::log_input/output/tool()` 审计 API，目标 target 为 `audit::input/output/tool`。

**技术实现**：
- 唯一 logger：`UnifiedLogger`（实现 `log::Log`），内部按 `record.target()` 前缀路由到 5 个文件 sink（aemeath/tui/hook/input/output/tool）。
- `UnifiedLogger::enabled()` 委托 `env_logger::Logger::enabled()`，保留 `RUST_LOG` + `config.level` 解析能力。
- 诊断 sink：每条 `Record` → `serde_json::to_string(&diag_record{ts, level, target, msg, ...})`（compact）写文件 → 一行。
- 审计 sink：通过静态方法（`UnifiedLogger::log_input(json)` 等）暴露，绕过宏直接写入：
  1. 包装 `{ts, session, chat, turn, model, ...payload}` 为 `serde_json::Value`
  2. `serde_json::to_string(&value)`（**compact**）→ 一行
- `panic.log` 维持 `panic_hook.rs` 现状，不纳入 `UnifiedLogger` 路由（panic 时 logger 自身可能不可用）。

**分歧记录（路径选择）**：
- 路径 A（双管线：RoutingLogger + JsonLogger 平行）：被否，调用点分裂、过滤逻辑重复。
- 路径 B（RoutingLogger 吞 JsonLogger，用 `log::info!(target: "audit::input", "{:#?}", json)`）：被否，`serde_json::Value` 退化为字符串、丧失结构化优势。
- **路径 C（JsonLogger 实现 `log::Log`，按 target 路由 + 审计 sink 静态方法）** = 选定：真正单入口、共享 `enabled()` 过滤、审计数据保留原始结构。

**改动文件**：
- `packages/global/logging/src/lib.rs` — 重新组织导出
- `packages/global/logging/src/multi_logger.rs` — **删除**（合并入 unified_logger）
- `packages/global/logging/src/json.rs` — **改造**为 unified_logger（实现 `log::Log` + 审计静态方法）
- `packages/global/logging/src/unified_logger.rs` — **新增**（或整合入 json.rs，由实现选择决定）
- `packages/global/logging/src/format.rs` — **新增**（诊断日志的 JSON 行格式化）
- `packages/global/logging/src/context.rs` — **新增**（SESSION_ID / CURRENT_CHAT_ID / CURRENT_TURN / CURRENT_MODEL 全局上下文）
- `agent/features/runtime/src/utils/bootstrap/logging_setup.rs` — 替换 `init_logging` 为 `UnifiedLogger::init(config)`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs` — `set_current_chat_id()` 调用
- `agent/features/runtime/src/business/agent/runner/setup.rs` — `set_current_model()` 调用
- `agent/features/hook/src/business/hook/runner.rs` — 5 处 `info` → `debug`
- `agent/features/runtime/src/utils/audit/**` — 调用点从 `JsonLogger::log_input(...)` 改为 `UnifiedLogger::log_input(...)`（如保留审计 API 命名）
- `specs/rust-coding.md` — 更新日志规范（统一入口、target 路由表、JSON 格式、职责边界）
- `docs/feature/active.md` — 本条目

**验证**：
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 启动 TUI，确认 `~/.agents/logs/` 下同时生成 `aemeath.log` + `tui.log`（JSON Lines 格式）
- 触发 hook，确认 `hook.log` 生成且常规日志为 debug 级别
- 触发一次 LLM 调用 + 一次 tool call：
  - `input.log` / `output.log` / `tool.log` 写入**单行** JSON（每行 = 1 个完整 JSON 对象，无 pretty-print 缩进）
  - 用 `wc -l input.log` 行数 = LLM 调用次数；用 `jq '.' input.log` 无 parse error
  - 用 `jq '.messages | length' input.log` 可下钻到嵌套数据
- 确认 `RUST_LOG=hook::runner=debug` 能让 `hook.log` 输出流水日志
- 确认 `role_logs_enabled=false` 时审计文件不生成内容
- 单元测试：`UnifiedLogger::enabled()` 转发 `env_logger` 过滤；`UnifiedLogger::log_input()` 在 `role_logs_enabled=false` 时短路返回；审计 sink 输出不含换行符（保证 JSON Lines）

### #80 Agent context 所有权重构（project 拥有 WorkspaceState）

**状态**：✅ 已完成（合并 commit `26dee4c5`），待确认

**设计 / 计划文档**：
- 设计：[docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md](../superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)
- 实施计划：[docs/superpowers/plans/2026-06-07-agent-context-ownership-redesign.md](../superpowers/plans/2026-06-07-agent-context-ownership-redesign.md)

**背景**：`agent/` 用 5 套类型表达同一组 workspace 事实（`ToolContext` 三字段、`ToolContextParts`、`WorktreeWorkingContext`、`share::tool::WorkingContext`、`WorkspaceContext`），导致：所有权弥散；`working_root`/`path_base` 两把独立 `Arc<Mutex>` 存在撕裂读；子 agent 经 `Arc::clone` 共享父 workspace，子 EnterWorktree 会改到父工作目录；worktree 业务直接内联 `Command::new("git")`（domain 直捅 infra）。

**目标 / 决策**：抛开旧设计「runtime 拥有 context」，改为 **project 切片拥有 workspace 类型与转换规则，runtime 仅持有实例生命周期**（依据实测依赖图：无 feature 能依赖 runtime）。

**已完成的实现**：
1. **唯一可变 owner**：`project` 内 `WorkspaceState { initial_cwd, working_root, path_base, stack }`，由 `WorkspaceService` 包一把锁，enter/exit/set_cwd 原子切换（修撕裂读）。
2. **三能力 trait（inbound port，定义在 project，被 tools/runtime 消费）**：`WorkspaceRead`（current_root/current_path_base/resolve）、`WorkspaceControl`（set_cwd/switch_to/enter/exit）、`WorkspacePersist`（snapshot/restore）。`switch_to` 为带存在性+同源校验的跳转（供 `ExitWorktree{path}`），保留原安全边界。
3. **git outbound port**：`GitWorktreeOps` + `GitCli`（git_common_dir/show_toplevel/in_worktree/worktree_add/current_branch），可注入 `FakeGit` 做纯单测；项目内 git spawn 收敛至 `git_ops.rs`。
4. **子 agent 隔离**：`WorkspaceService::seed_isolated()` 从父当前快照派生独立实例（继承 root/base、空栈、独立锁），修复共享父 workspace 的 bug；`agent_semaphore` 仍共享。
5. **`ToolContext` → `ToolExecutionContext`**：删除 `working_root`/`path_base`/`context_stack` 三字段，改持 `Arc<WorkspaceService>` + `workspace_read()`/`workspace_control()` 访问器。
6. **runtime client 跨 chat 轮次持有 `WorkspaceService`**，取代 `inner.workspace_context` 与 per-loop seed；session 保存/恢复走 `snapshot()`/`restore()`（serde 字段不变，旧 session 兼容）。
7. **退役**：`WorktreeContextExt`、`ToolContextParts`/`build_tool_context`、`ProjectGateway`/`DefaultProjectGateway`、`share::tool::WorkingContext`、旧 `worktree.rs`/`working_paths.rs`。
8. **防回归 guard**：新增 `.agents/hooks/check-context-architecture.sh`（R1–R6：ToolExecutionContext 无三字段、tools 不引用持久化 DTO、WorkspaceState 仅 project、`workspace_control()` 仅 bash/worktree 工具、project git 仅 git_ops.rs、WorkspacePersist 仅 project+runtime），接入 `check-architecture-guards.sh`。

**关键修正（实施期发现）**：bash `cd` 也是 workspace mutator（纳入 `set_cwd`）；git spawn 禁入 share（GitCli 全在 project）；`ExitWorktree{path}` 经 `switch_to` 恢复存在性+同源校验（避免在 LLM 可控输入上削弱安全边界）。

**验证**（合并入 main 后在 main 复验）：`cargo test --workspace` 1387 通过；`cargo clippy --workspace --all-targets -- -D warnings` 无问题；`check-architecture-guards.sh` 全部通过（含新 context guard）。

**涉及路径**：
- `agent/features/project/src/business/{workspace_state,workspace_service,workspace_types,git_ops}.rs`、`contract.rs`、`api.rs`
- `agent/features/tools/src/contract/context.rs`、`contract.rs`、`business/{worktree,bash,file_read,file_edit,file_write,glob_tool,grep,lsp,agent_tool}.rs`
- `agent/features/runtime/src/core/client/{accessors,from_args,trait_chat,trait_session,trait_accessor,event,mapping}.rs`、`business/chat/looping/{loop_runner,agent_calls,non_agent,post_batch}.rs`、`business/agent/runner/setup.rs`
- `agent/composition/src/{app,lib}.rs`、`agent/shared/src/{session_types,tool}.rs`
- `.agents/hooks/check-context-architecture.sh`、`.agents/hooks/check-architecture-guards.sh`、`.agents/hooks/check-crate-api-boundary.sh`

### #81 TUI assistant 文本与 spinner phase 视觉调整

**状态**：待确认

**症状 / 目标**：spinner phase 文案前的 emoji 造成状态行视觉噪音；assistant 正文当前没有行首标识，和周边 block 的层级提示不一致。目标是 spinner phase 只显示纯文本，同时 assistant 文本首行前显示白色圆点 gutter。

**根因 / 设计点**：
1. Spinner phase 文案集中在 `LiveStatusAssembler::phase_text()`，该函数直接拼入 emoji 前缀。
2. 输出区 gutter 的 marker 由 `marker_glyph()` 按 `OutputBlockKind` 映射；`AssistantMessage` 当前落入默认空 marker。

**实现**：
1. 将 spinner phase 文案改为纯文本：`Thinking...`、`Generating...`、`Calling <tool>...`、`Hook <event>...`。
2. 为 `OutputBlockKind::AssistantMessage` 映射静态 `●` marker，颜色使用 `theme::ASSISTANT`。
3. 保持 gutter 只进入 spans、不进入 plain；续行继续使用等宽空白，不影响复制和选区坐标。

**验证**：
- `CARGO_TARGET_DIR=target cargo test -p cli live_status`
- `CARGO_TARGET_DIR=target cargo test -p cli gutter`
- `cargo fmt --check`
- `CARGO_TARGET_DIR=target cargo check -p cli`
- `CARGO_TARGET_DIR=target cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/view_assembler/live_status.rs`
- `apps/cli/src/tui/adapter/live_status_widget.rs`
- `apps/cli/src/tui/render/output/gutter.rs`

### #82 Provider/Config 设计债收口

**状态**：未开始（2026-06-10 设计分析产出，待方案确认后实施）

**背景**：对 `agent/features/provider/**` 与 `agent/shared/src/config/models/**` 的设计分析确认整体分层方向正确：config 声明（`ModelsConfig` 的 source + driver 两级模型，source 自由命名、driver 封闭枚举）→ bootstrap 合并翻译（CLI/env/config 优先级合并、`ReasoningConfig` 决策树）→ `LlmClient` 门面（集中可观测性）→ driver 协议差异（`ChatApiDriver` strategy trait 吸收 OpenAI/Zhipu/LiteLLM/Volcengine 的请求字段差异：`max_tokens_field()` + `apply_reasoning_fields()`；流解析以宽容超集方式同时识别 `content` / `reasoning_content` 吸收响应差异）。问题集中在 `LlmClientPool::create_client` 旁路绕过 bootstrap 层自行实现解析，与主路径行为分叉。

**问题清单**：

1. **API key 解析双实现、优先级冲突**：主路径 `runtime/src/utils/bootstrap/provider_client.rs` 为 CLI → `AEMEATH_API_KEY` → driver 专属 env → `LLM_API_KEY` → config（env 优先于 config）；pool 路径 `provider/src/core/pool.rs::create_client` 为 config 优先于 env，不识别 `AEMEATH_API_KEY`，多出 `OPENAI_API_KEY` 兜底。driver→env 名映射表在两处重复定义。同一份配置下主 client 与子 agent client 可能使用不同 key，违反 DRY。
2. **pool 创建的 client 丢失 reasoning 配置**：`pool.rs::create_client` 硬编码 `reasoning = true`、`reasoning_config: None`，忽略 `model_entry.reasoning / reasoning_effort`，子 agent 不遵循模型级 reasoning 设置，与主路径决策树不一致。
3. **未知 driver 静默 fallback OpenAI**：`pool.rs` 与 `from_args.rs` 均为 `ProviderDriverKind::parse(...).unwrap_or(OpenAI)`，配置写错 driver 名不报错；与已归档 bug #85（Ollama 工厂未接线）同源。
4. **spec 漂移**：`specs/provider.md` 支持列表（OpenRouter/DeepSeek/Moonshot/DashScope/MiniMax 等）与 `ProviderDriverKind` 实际枚举（Anthropic/OpenAI/Zhipu/LiteLLM/Volcengine/Ollama）不一致——前者是"经 openai driver 可配置的厂商"，后者才是代码事实；`specs/config-compat.md` 所述 provider 默认值位置与实际（仅 Volcengine 有内置默认 source）不符。
5. **默认值与 legacy 字段散落**：`200000` max_tokens 兜底在 `client.rs` 与 `pool.rs` 各一份；user-agent 字符串在 `config/legacy.rs` 与 `openai_compatible/provider.rs` 各 format 一次；`max_retries: 10` 硬编码而 legacy `ApiConfig.timeout/retries` 未接入新路径。
6. **死代码**：`OpenAIProviderConfig::from_driver` 为 Anthropic driver 配置的 `/v1/messages` suffix 分支不可达（Anthropic 不走 OpenAI 兼容路径）。

**修复方向**：

1. 将 `resolve_api_key` / `reasoning_config` 决策下沉到 pool 与 bootstrap 共同可依赖的位置，`LlmClientPool::create_client` 复用同一套解析，消除两套优先级。
2. `ProviderDriverKind::parse` 失败时显式报错（附可用 driver 列表），移除 `unwrap_or(OpenAI)` 静默降级。
3. 散落默认值收口为单一常量来源；清理不可达 suffix 分支；决定 legacy `ApiConfig.timeout/retries` 接入或删除。
4. 同步修订 `specs/provider.md` / `specs/config-compat.md`（spec 修改前需用户同意）。

**验证**：
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 回归覆盖：unknown driver 报错路径、pool 与主路径 key 解析优先级一致性、bug #85 场景（ollama driver 路由）

**涉及路径**：
- `agent/features/provider/src/core/pool.rs`、`core/client.rs`、`contract.rs`
- `agent/features/runtime/src/utils/bootstrap/provider_client.rs`
- `agent/shared/src/config/models/types.rs`、`config/legacy.rs`
- `specs/provider.md`、`specs/config-compat.md`

### #83 Tool result 统一结构化 JSON

**状态**：实现中

**当前进展**：
- `ToolResult` 已增加 `content: serde_json::Value`，`success/error` 默认包装为 `{ "text": ... }`，并保留 `output` 作为 TUI / legacy fallback。
- runtime / sdk / storage oversized 持久化路径已携带 JSON content，`Message::tool_results_rich` 无图片时直接写入结构化 content，有图片时附带 text 与 json block。
- TUI `UiEvent`、conversation model 与 view assembler 已接入结构化 content；`EnterWorktree` / `ExitWorktree` 优先展示 `message` 与 `当前分支：{branch}`，解析失败回退文本。
- `EnterWorktree` / `ExitWorktree` 已返回 `status/message/branch/path_base/working_root/guidance` JSON schema；其他现有工具通过默认构造器统一包装为 `{ "text": ... }`。

**症状 / 目标**：当前工具执行结果在 `ToolResult.output`、runtime stream event、TUI conversation model 中主要以纯文本 `String` 流转；虽然共享消息层的 `ContentBlock::ToolResult.content` 已支持 `serde_json::Value`，但工具层没有统一结构化 payload，导致 LLM 只能收到非结构化文本，TUI 也只能按行截断或做工具名特判。目标是所有 tool result 统一返回 JSON payload：LLM 获得完整结构，TUI 可按工具选择字段展示。

**根因 / 设计点**：
1. `agent/shared/src/tool.rs` 的 `ToolResult` 以 `output: String` 为主，缺少结构化字段。
2. runtime 在 `RuntimeStreamEvent::ToolResult` / `UiToolResult` / `Message::tool_results_rich` 路径中把结果退化为文本。
3. provider conversion 对非 String `content` 已具备 stringify fallback，但当前工具层没有稳定 JSON schema 可依赖。
4. TUI 的 `ToolResultBlockView.result_text` 只保存字符串，缺少按字段展示的统一解析入口。

**实现方向**：
1. 为 `ToolResult` 增加统一结构化 JSON payload，并保留文本 fallback，所有现有工具默认映射为 `{ "text": "..." }`，避免一次性破坏兼容性。
2. 修改 runtime 消息流和发给 LLM 的 tool result 构造逻辑，使 LLM 看到结构化 JSON；provider 层按各 API 能力使用原生 JSON 或 JSON string。
3. 改造所有内置工具返回明确 JSON schema；通用字段建议包括 `status`、`message`、`data`、`diagnostics`、`display`，工具专属字段放入 `data`。
4. `EnterWorktree` / `ExitWorktree` 优先落地结构化 result：保留 `message`、`branch`、`path_base`、`working_root`、路径使用 guidance；TUI 仅展示 `message` 与 `当前分支：{branch}`。
5. TUI 增加结构化 result 展示选择层：优先读取 JSON 中的 display 字段或工具专属字段，解析失败时回退现有纯文本渲染。
6. 更新 session/history/storage 中 tool result 持久化兼容逻辑，确保旧会话纯文本 result 可继续 resume。

**验证**：
- 增加共享 `ToolResult` JSON serialization / fallback 单元测试。
- 增加 runtime tool result → LLM message 的结构化 content 测试。
- 增加 TUI 对结构化 worktree result 的字段选择渲染测试。
- 对代表性工具（文件、bash、搜索、任务、agent、worktree）补充 result JSON schema 回归测试。
- 运行 `cargo fmt --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。

**涉及路径**：
- `agent/shared/src/tool.rs`
- `agent/shared/src/message/*`
- `agent/features/runtime/src/business/chat/looping/*`
- `agent/features/tools/src/**`
- `agent/features/provider/src/business/providers/**/message_conversion.rs`
- `packages/sdk/src/tui.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/tool_call.rs`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/render/output/**`

### #84 Stop hook 命令显示短路径

**状态**：✅ 已完成

**背景 / 目标**：Stop hook 在 TUI 中显示执行命令时，当前会展示类似 `{AEMEATH_PROJECT_DIR}/build_cli.sh` 的完整模板路径，内容过长且项目路径变量对用户定位脚本帮助有限。目标是在用户可见的 hook stop 提示中只显示最后一级命令/脚本名，例如 `build_cli.sh`。

**设计方向**：
1. 仅调整 TUI/用户可见展示文本，不改变 hook 实际执行命令、环境变量注入或日志中的原始命令。
2. 对 hook 命令展示做路径 basename 化：包含 `/` 的命令只取最后一段；不含 `/` 的命令保持原样。
3. 需要覆盖 `{AEMEATH_PROJECT_DIR}/build_cli.sh`、绝对路径、相对路径、普通命令名等场景；必要时保留 tooltip/detail 或日志里的完整命令用于排查。

**验收标准**：
1. Stop hook 运行/阻止提示中显示 `build_cli.sh`，不再直接显示 `{AEMEATH_PROJECT_DIR}/build_cli.sh` 这类长路径。
2. Hook 执行行为不变，`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 注入不受影响。
3. 完整命令仍可在日志或内部结果中用于调试，不因展示缩短而丢失执行信息。

**验证**：
- `CARGO_TARGET_DIR=target cargo test -p cli hook_notice`
- `CARGO_TARGET_DIR=target cargo check -p cli`
- `CARGO_TARGET_DIR=target cargo clippy -p cli --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

**涉及路径**：
- `apps/cli/src/tui/**`
- `agent/features/hook/src/business/hook/**`

### #85 Read/TaskList tool call 单行摘要展示

**状态**：待确认

**目标**：简化 Read、TaskListCreate、TaskUpdate 等工具调用在 TUI 中的展示，让工具调用 header 和 result 区域更紧凑，减少视觉噪音；非失败场景只显示单行摘要，失败详情保留在 result 区域展开。

**设计方向**：

1. **Read tool call 单行展示**：
   - 成功时 header 只显示读取范围（如 `Read src/main.rs: L12-L34`）和总行数（如 `23 lines`）。
   - 失败时 header 只显示工具名与文件路径，具体错误原因在 result 区域展示。
   - 不展示完整文件内容摘要或多余前缀。

2. **TaskListCreate / TaskUpdate 单行展示**：
   - TaskListCreate 成功时 header 显示 `TaskListCreate: N tasks`，单行概括创建了多少任务。
   - TaskUpdate 成功时 header 显示 `TaskUpdate: task #N → <status>`，单行概括更新动作。
   - 失败时同样只保留工具名与简要动作，原因展开在 result 中。

3. **失败原因统一收敛到 result 区域**：
   - 当前部分工具在 header 或 gutter 中即展示错误信息，导致换行或过长；统一改为 result 中展开。
   - 保持 TUI 对 tool result 的结构化展示能力（Feature #83 基础），优先读取 JSON `message` / `status` 字段。

**验收标准**：
1. Read 工具调用 header 为一行，含文件路径与读取范围/行数。
2. TaskListCreate / TaskUpdate header 为一行，含任务数或状态变化。
3. 失败场景 header 不展开错误详情，错误信息可在 result 区域完整查看。
4. 不破坏现有 tool call 的 gutter marker、选中、复制行为。
5. 编译通过，`cargo test --workspace` 无回归。

**验证**：
- `cargo test -p cli tool_call -- --nocapture`
- `cargo test -p cli read_tool -- --nocapture`
- `cargo test -p cli task_list -- --nocapture`
- `cargo fmt --check`
- `cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/render/output/blocks/tool_call.rs`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/model/conversation/tool_call.rs`
- `agent/features/tools/src/file_read.rs`
- `agent/features/tools/src/task_list.rs`

### #86 粘贴长文本自动折叠为 `[Copied Text n]`

**状态**：待确认

**目标**：当用户向 TUI input area 粘贴超过两行的文本时，不直接展开显示完整内容，而是折叠为 `[Copied Text n]`（n 为本次会话中第几次发生长文本粘贴）。同样，当该输入进入 input queue 时，也以折叠形式展示；TUI 输出区对这类折叠标记原样渲染，保持界面整洁。

**设计方向**：

1. **粘贴检测与折叠**：
   - 在 input area 粘贴路径检测插入文本是否包含超过两个 `\n`（即超过两行）。
   - 若超过两行，将实际文本暂存到会话级或输入模型级缓冲区，并在 input area 中显示 `[Copied Text n]` 占位符。
   - `n` 为本次会话中累计的长文本粘贴次数，从 1 开始递增。

2. **Input queue 一致性**：
   - 当用户提交含折叠占位符的输入时，实际发送给 LLM 的内容仍是原始完整文本。
   - input queue 中显示时同样以 `[Copied Text n]` 占位，避免队列区域被长文本撑开。

3. **TUI 输出区原样渲染**：
   - 对用户消息中的 `[Copied Text n]` 占位符不做 Markdown 解析，按纯文本原样渲染。
   - 选中和复制行为保持可用；复制时应复制占位符文本（与显示一致）。

4. **边界场景**：
   - 单行或两行粘贴不触发折叠，按现有行为直接显示。
   - 编辑态下删除占位符视为删除对应粘贴内容。
   - 多次粘贴各自独立计数和占位。

**验收标准**：
1. 超过两行的粘贴文本在 input area 显示为 `[Copied Text n]`。
2. 提交后进入 input queue 同样显示折叠占位符。
3. TUI 输出区对用户消息中的占位符原样渲染、不解析 Markdown。
4. 实际发送给 LLM 的内容为原始完整文本，不影响对话语义。
5. 不破坏现有单行/双行粘贴、输入历史、撤销/重做行为。

**验证**：
- `cargo test -p cli input_area -- --nocapture`
- `cargo test -p cli input_queue -- --nocapture`
- `cargo test -p cli paste -- --nocapture`
- `cargo fmt --check`
- `cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/input_area.rs`
- `apps/cli/src/tui/input_queue.rs`（如存在）
- `apps/cli/src/tui/model/input.rs`（如存在）
- `apps/cli/src/tui/app/update.rs`（粘贴处理）
- `apps/cli/src/tui/render/output/blocks/user_message.rs` 或通用文本渲染路径
