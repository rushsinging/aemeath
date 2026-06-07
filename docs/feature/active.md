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
| 75 | EnterWorktree/ExitWorktree result 不截断 | 低 | 待确认 | 未确认 | 为 worktree 工具单独放宽 result 展示行数，不再被截断 |
| 77 | diff removed 行不语法高亮，只显示纯红色 | 低 | 待确认 | 未确认 | removed 行改为纯 `DIFF_REMOVE_FG` 红色，不调用语法高亮 |
| 78 | CLI 增加 `-q` 无 TUI 模式和 `-v` 日志输出到 stderr 模式 | 中 | 活动中 | 未确认 | `-q` 跳过 TUI 直接 REPL，`-v` 日志输出到 stderr |
| 79 | 日志模块整理与 hook 可观测性增强 | 中 | 待确认 | 待用户确认 | 移除废弃 `module_levels`，统一全局过滤；补充 hook 初始化/匹配/分发日志 |

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
| PostCompact 提取记忆 | 已改为 Compact 前基于即将被压缩的 early messages 提取建议 |
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
| Prompt 模板（偏差检测 + 建议记忆 + 过时记忆） | `reflection/prompt.rs` |
| `/reflect` 命令（后台异步触发 + apply/stats/history） | `tui/app/slash/reflection.rs` |
| pending 建议与 `/reflect apply` 写入 MemoryStore | `tui/app/slash/reflection.rs` |
| `auto_apply_suggestions` 自动应用 suggested memories 与 outdated markers | `tui/app/update/ui_event.rs` |
| 自动 N 轮触发（`reflection.interval_turns`，0 禁用） | `chat/looping/reflection.rs` |
| Compact 前基于 early messages 提取 memory 建议，默认不自动写入 | `chat/looping/compact.rs`、`compact/summary.rs` |

**暂缓**：连续工具失败触发、SessionEnd/SubagentStop 反思、独立 reflection model、PostCompact 后反思（已改为 Compact 前建议提取）。

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

**状态**：活动中

**背景**：Stop hook 阻止结束时，反馈既需要作为 `<system-reminder>` 注入下一轮 LLM 上下文，也需要在 TUI 中提示用户。但当前用户可见提示沿用普通 `SystemMessage`/`SystemNotice` 展示，视觉上接近用户输入；部分场景还可能把 `<system-reminder>` 标签原样展示，造成用户误以为系统内部标签进入了可见对话内容。后续 StopFailure 或其他 hook 也可能产生不同语义的用户可见消息，继续复用普通 SystemMessage 会让样式、文案和脱壳规则分散。

**目标**：
1. TUI 展示层对 `<system-reminder>...</system-reminder>` 包装做统一脱壳，只展示内部的人类可读内容。
2. 新增 Hook 类用户可见消息，避免 Hook 反馈混用普通 SystemNotice；Hook 消息应能承载来源事件或语义类型，便于 Stop、StopFailure 和未来 Hook 使用不同文案/样式。
3. 保留 LLM 上下文注入中的 `<system-reminder>` 语义，不改变模型可见系统提醒协议；脱壳仅作用于 TUI 可见展示。
4. Hook 阻止类消息在视觉上应与用户输入明显区分，优先使用 warning/error 语义色或明确前缀。

**建议实现方向**：
1. 在 runtime/SDK 事件层为 Hook 反馈引入类型化事件或 payload，而不是仅传 `SystemMessage(String)`；短期可先兼容旧 SystemMessage。
2. 在 TUI adapter/model 层新增 `ConversationBlock::HookNotice` 或等价 block，集中处理 Hook notice 文案、样式和脱壳。
3. 抽出一个单一 helper 负责剥离完整包裹的 `<system-reminder>` 标签，避免在多个渲染点重复字符串处理。
4. Stop hook blocked、StopFailure hook output、未来 hook JSON `system_message`/`additional_context` 的用户可见路径统一走 HookNotice。

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

**涉及路径（预计）**：
- `agent/features/runtime/src/business/chat/looping/finalize.rs`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- `packages/sdk/src/*`（如需新增 ChatEvent 类型）
- `apps/cli/src/tui/effect/session/processing.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/*`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/render/output/blocks/diagnostic.rs` 或新增 Hook notice renderer

### #75 EnterWorktree/ExitWorktree result 不截断

**状态**：待确认

**背景**：EnterWorktree / ExitWorktree 的工具结果是固定的工作区上下文提示，通常只有少量行；默认 `TOOL_RESULT_MAX_LINES = 5` 会导致 TUI 输出区显示 `... (n lines omitted)`，隐藏后续关于 path_base / working_root 使用约束的关键提示。

**实现**：
1. 保持全局默认工具结果预览行数不变，避免影响 Bash / Read / Grep 等可能产生大输出的工具。
2. 为 `EnterWorktreeDisplay` 与 `ExitWorktreeDisplay` 单独覆盖 `result_max_lines()`，允许完整展示固定上下文结果。
3. 新增回归测试覆盖 EnterWorktree / ExitWorktree 结果不出现 `lines omitted`，且仍展示最后一条工作区路径使用提示。

**验证**：
- `cargo test -p cli test_render_tool_result_worktree_tools_do_not_truncate_fixed_context_result`
- `cargo test -p cli tool_result`
- `cargo fmt --check`
- `cargo check -p cli`

**涉及路径**：
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- `apps/cli/src/tui/render/output/blocks/tool_result.rs`

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
