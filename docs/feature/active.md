# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 4 | AskUserQuestion TUI 美化 | - | 待实施 | 未确认 | AskUserQuestion 等待用户输入时，在 output area 提示「请在下方输入区域输入」；操作指引（[Enter] 确认 / [Esc] 取消 / [Tab] 切换选项）放在提示文字末尾，不单独占行；纯选择模式（仅 options、无 free_input）改为上下键高亮选项 + Enter 确认 |
| 8 | Memory 系统 | - | 实施中 | 未确认 | MVP 已落地：MemoryConfig、MemoryStore、/memory 命令、MemoryTool、system prompt 注入；本轮补齐注入配置化与对话结束后的 session reminder recap。Hook 兜底自动提取与淘汰确认暂缓。详见 [spec](specs/008-memory-system.md) |
| 9 | 反思系统 | - | 实施中 | 未确认 | 已接入真实 LLM `/reflect`、JSON 解析、pending 建议与 `/reflect apply` 写入 Memory、auto_apply_suggestions 自动写入、自动 N 轮触发；使用当前默认模型，不做独立 reflection model；不做 PostCompact 后反思，避免压缩后上下文损失。详见 [spec](specs/009-reflection-system.md) |
| 28 | MCP 系统完善 | 高 | 🔧 未完成 | 未确认 | P0+P1 已完成：stdio 可用配置、配置层、Manager/API、命令解析、工具注册/注销和默认 1MB tool result 限制已落地；SSE 传输已实现但存在可靠性问题（z.ai SSE server 响应在 tools/list 时经常超时/不完整），MCP 加载已暂时从启动流程中禁用，待修复后重新启用；Streamable HTTP 传输待后续补充 |
| 35 | Diff 渲染中 add 行语法高亮 | 中 | 待实施 | 未确认 | LLM 输出的 unified diff 中，`+` 开头的行（add 内容）按目标文件语言做语法高亮，提升代码变更可读性 |
| 34 | Anthropic Claude 原生 Provider | 高 | ✅ 已完成 | 未确认 | 原生 Anthropic Claude API 适配（Messages API、流式/非流式、thinking budget、重试、tool use），作为独立 provider 与 OpenAI/OpenRouter 等并列；默认 provider |
| 36 | Multi-Agent 框架 | 高 | 实施中 | 未确认 | Sprint 0/Sprint 0.5 已完成；Sprint 1 已完成 Workspace/Chat API 收尾切片：REST Workspace/Chat/Message、GET messages 分页、PATCH Chat、REST AnalyzeMessage、WebSocket payload、gateway 配置、idempotency_key 去重与 server 内部 EventBus 骨架；当前架构文档已改为 Server + Agent 分布式部署：MongoDB 作为业务状态真相源，Redis Streams 作为消息层，取消 Agent RPC / Watch 接口，调度改为 WorkItem + lease + OutboxPublisher；Sprint 2 UI 已收敛为全屏 Sticky Whiteboard Workbench，选定静态 mockup `docs/feature/mockups/036-sprint-2-sticky-whiteboard-workbench.html`，并新增 `ui/` Vue 3 + Vite + Pinia + Element Plus 核心骨架：左右抽屉、紧凑便利贴、Requirement/Project 状态、agent 标识、点击详情和底部 chat dock 已用静态 Board store 落地；下一步接入 `@aemeath/sdk` REST/WS 真实 BoardEvent，或继续补 MongoDB 持久化、Redis WorkQueue/BoardEvent 与 Outbox。详见 [spec](specs/036-01-plan.md) |
| 37 | 火山引擎（Volcengine）Coding Plan Provider | 高 | 待确认 | 未确认 | 新增 `volcengine` ApiDriverKind，复用 OpenAI-compatible Provider；2026-05-20 修正 Volcengine 请求体 max token 字段为 `max_output_tokens`，避免错误发送 `max_tokens` |
| 38 | TUI 日志文件（`~/.agents/logs/tui.log`） | 中 | 待实施 | 未确认 | 新增 TUI 专属日志类别 `tui.log`，与现有 `aemeath.log`/`agent.log` 并列存放于 `~/.agents/logs/`，记录 TUI 事件循环、渲染、输入处理、选区/复制等 UI 层行为，方便排查 TUI 相关 bug（如 #42 乱码、#48 选中错位） |
| 39 | TUI 配色方案重新设计 | 中 | 待确认 | 未确认 | 已新增集中 TUI theme，按 Claude Code / 现代 IDE 风格统一输出区、Markdown、spinner、task list、输入区、状态栏、补全面板、dialog、快捷键帮助的语义配色；2026-05-21 修正 status line 使用专用背景色，避免近黑底；不引入运行时主题切换、不改布局、不引入外部配置 |
| 40 | 配置文件改造：对齐 Codex 风格的 `~/.agents` / `AGENTS.md` / skills 读取 | 高 | 待确认 | 未确认 | 将当前 `.aemeath` / `CLAUDE.md` / skill 读取链路调整为 Codex 方向：全局配置根默认迁移到 `~/.agents` 且可配置，agent 配置文件使用 `aemeath.json`；指令读取 `~/.agents/AGENTS.md` 与 `{cwd}/AGENTS.md`；skills 读取 `~/.agents/skills` 与 `{cwd}/.agents/skills`；实现时必须考虑 git worktree 下 repo root / checkout root / cwd 的配置继承与去重，并由本次更新在发布/部署阶段手动迁移现有 `~/.aemeath` 数据。 |

### #40 配置文件改造：对齐 Codex 风格的 `~/.agents` / `AGENTS.md` / skills 读取

**目标**：把 Aemeath 的配置、项目指令和 skills 发现机制调整为 Codex 风格，统一围绕 `~/.agents` 与项目内 `.agents` 目录组织，降低与 Claude 专属路径的耦合，并支持把当前用户已有配置迁移到最新模式。

**核心要求**：

**实现结果（2026-05-21）**：运行时读取已切换到新路径：全局配置 `~/.agents/aemeath.json`，项目配置 `{cwd}/.agents/aemeath.json`；指令 `~/.agents/AGENTS.md` 与 `{cwd}/AGENTS.md`；skills `~/.agents/skills` 与 `{cwd}/.agents/skills`；应用主日志 `~/.agents/logs/aemeath.log`。程序不提供 `/config migrate` 且启动时不自动迁移；现有 `~/.aemeath`、`~/.claude/CLAUDE.md`、项目 `CLAUDE.md` 与旧 skills 由本次更新在发布/部署阶段一次性手动复制到新路径。Worktree 下以启动 `cwd` 为边界读取，不跨 checkout 共享项目配置。

1. **全局配置目录**：默认使用 `~/.agents` 作为全局配置根，且该根目录本身必须可配置。
2. **Agent 配置文件**：Aemeath 的主配置文件改为 `aemeath.json`，默认路径为 `~/.agents/aemeath.json`；项目级配置后续可按 Codex 风格放在项目 `.agents` / `.codex` 同类目录中评估，但本 feature 的明确目标是先统一全局配置与 agent 配置命名。
3. **AGENTS.md 指令读取**：读取 `~/.agents/AGENTS.md` 与 `{cwd}/AGENTS.md`，用于替代当前 `CLAUDE.md` 方向；拼接顺序、冲突处理和 prompt injection 扫描需在实现前明确。
4. **skills 读取目录**：skills 固定优先读取 `~/.agents/skills` 与 `{cwd}/.agents/skills`，保持 skill 包与命名空间机制可用。
5. **现有配置迁移**：实现时必须提供从当前模式迁移到新模式的路径，包括但不限于 `~/.aemeath/config.json`、`{cwd}/.aemeath/config.json`、`~/.aemeath/skills`、`{cwd}/.aemeath/skills`、`{cwd}/CLAUDE.md`、`~/.claude/CLAUDE.md` 到新路径的迁移、兼容读取或提示方案。
6. **Worktree 场景**：必须考虑 git worktree。读取项目指令和 repo skills 时，不能只假设 `cwd` 就是主 checkout；需要明确 cwd、worktree checkout root、git common dir / repo root 的关系，避免 linked worktree 中漏读项目级 `.agents/skills` 或重复读取同一份配置。

**推荐设计方向**：

- 新增统一的配置根解析逻辑：默认 `~/.agents`，允许通过 CLI 参数、环境变量或现有配置 bootstrap 指定其他目录；解析后统一供配置、skills、AGENTS.md 使用，避免各模块重复拼路径。
- 新增迁移/兼容层：首次发现旧配置但新配置不存在时，提示或自动迁移到 `~/.agents/aemeath.json`；迁移完成前可保留只读兼容窗口，但新写入必须写到新路径。
- 项目指令读取从 `CLAUDE.md` 改为 `AGENTS.md`：全局 `~/.agents/AGENTS.md` + 当前工作目录 `{cwd}/AGENTS.md`。后续如需 Codex 的父目录链式读取，应单独拆 feature，避免本轮范围失控。
- Skills 读取收敛到 `~/.agents/skills` 与 `{cwd}/.agents/skills`；旧目录仅作为迁移来源，不应继续作为长期主路径。
- Worktree 处理应优先基于 git 信息确定 worktree checkout root，并在 cwd 与 checkout root 不同时定义清楚读取策略：至少保证当前 cwd 的 `{cwd}/AGENTS.md`、`{cwd}/.agents/skills` 可用；如后续引入 checkout root 级读取，必须做路径 canonicalize 与去重。

**涉及路径（预计）**：

- `packages/core/src/config/manager/mod.rs`：配置文件路径与加载层级调整。
- `packages/core/src/config/mod.rs`：配置 schema / 默认路径常量调整。
- `apps/cli/src/prompt.rs`：`CLAUDE.md` 读取迁移到 `AGENTS.md`。
- `packages/core/src/skill/loader.rs`：skill root 调整为 `~/.agents/skills` 与 `{cwd}/.agents/skills`。
- `packages/core/src/state/settings.rs`：若仍保留 settings 写入，需迁移到新配置根或明确废弃。
- `docs/feature/active.md`：实现进度同步更新。

**验收标准**：

1. 新安装用户默认读取 `~/.agents/aemeath.json`、`~/.agents/AGENTS.md`、`~/.agents/skills`。
2. 项目内默认读取 `{cwd}/AGENTS.md` 与 `{cwd}/.agents/skills`。
3. 旧 `~/.aemeath/config.json` / `.aemeath/config.json` / `.aemeath/skills` / `CLAUDE.md` 存在时，有清晰迁移或兼容提示，不静默丢配置。
4. 在 linked worktree 中启动时，项目级 AGENTS.md 与 skills 读取行为稳定、可预测，且不会重复加载同一路径。
5. 新写入的配置落到新模式，不再写回旧路径。

---

### #36 Multi-Agent 框架

详见 [设计文档](specs/036-multi-agent-framework.md)

---

### #33 优化 TaskListCreate / TaskListComplete 工具调用显示（已归档 2026-05-14）
用户确认完成。详见 `docs/feature/archived/033-task-list-display-optimization.md`。

### #30 Agent loop 收尾工作

**目标**：把 agent loop 的所有退出路径收敛到统一 finalize，避免正常结束、用户打断、API 错误、超时、达到 max turns 等分支各自手写清理逻辑，导致 task 状态、hook、日志、session 持久化行为不一致。

**与 #27/#29/#34 的关系**：
- #27 已先在 `Agent` tool 层补 taskId 状态桥接，但子代理内部取消/超时/API error 等结果仍应由统一 `AgentRunOutcome` 表达，避免继续依赖字符串结果判断。
- #29 已先通过 prompt 和工具描述强化主 agent 必须 `TaskUpdate`，后续 #30 只做收尾检查和日志提示，**不应**启发式自动替用户推进 task 状态。
- #34 已先引入 task batch summary、`TaskListCreate` / `TaskListComplete` 和 reminder 隔离；后续 #30 可在 finalize 中检查 active list 是否仍有 pending/in_progress，并决定记录摘要或提示关闭，不自动误归档。

**推荐 P0 范围**：
1. 新增 `AgentRunStatus` / `AgentRunOutcome`，覆盖 completed、cancelled、timed_out、api_error、max_turns。
2. 将 `CliAgentRunner::run_agent()` 多个 early return 收敛为统一 finalize 函数/guard。
3. finalize 统一执行：恢复 client 设置、调用 `SubagentStop` hook、写结构化日志摘要（status、turns、duration、role、model）。
4. 保持当前对外行为不变；不在本 feature 中自动完成 pending task。
5. 补单元测试覆盖各类 outcome 的 finalize 行为，至少覆盖正常完成、错误、max turns。

**后续 P1/P2 可选**：
- session 持久化接入 finalize。
- tool 资源释放/取消 token 清理接入 finalize。
- task list 收尾检查结果写入日志或 reminder 状态。

**明确不做**：
- 不自动把所有 pending task 标记 completed。
- 不按工具调用启发式猜测应该更新哪个 task。
- 不在当前 #27/#29/#34 修复分支里扩大实现该 feature；本轮只记录设计，后续单独在 #30 中统一完成。

---

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

### #21 TUI 优化 Agent 调用输出展示

**目标**：优化 Agent 子任务每个 turn 的工具调用进度展示，避免只显示 `Read, Read, Grep` 这类无目标列表。

**已完成的改动**：

1. **结构化事件协议**：Agent progress 从 `Sender<String>` 升级为 `Sender<AgentProgressEvent>`，不再依赖 TUI 解析 `[Turn N]` 文本。
2. **工具调用摘要**：Agent runner 根据 tool call input 生成 `AgentToolCallProgress.summary`，例如 `Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src`。
3. **同工具分组**：TUI 根据结构化 calls 按工具名合并，并显示调用次数；turn/sequence 仅用于内部定位，默认不展示。
4. **当前进度单行更新**：同一个 Agent tool 的 `ToolCalls` 进度只保留一行，新事件替换旧行，不重复刷屏。
5. **兼容保留**：`AgentProgressKind::Message` 用于普通文本 progress，仍按原逻辑追加和去重。

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（Agent tool call progress 摘要生成）
- `aemeath-cli/src/tui/output_area/tool_display.rs`（同 turn progress 替换）

**测试**：新增单元测试覆盖结构化事件构造、目标摘要生成、同 Agent 当前进度替换、不同 Agent 互不覆盖、普通 Message progress 兼容。

---

### #23 TUI 字符串/切片安全索引收口

**目标**：把 TUI 路径中"按字符索引、按字节切片、按宽度截断、按显示列号定位"等容易越界的操作收口到一个统一的工具模块，业务路径全部走该模块的 API，禁止直接 `chars[from..to]`、`s[i..j]`、`chars().nth(n)`、`text.len()` 当字符长度等高风险写法。配合单元测试覆盖边界条件，根治"TUI streaming/选中/复制/渲染"路径反复出现的越界 panic。

**已完成的改动**：

1. 新增 `aemeath-cli/src/tui/safe_text.rs`，统一提供 panic-free 字符范围、字符串切片、显示宽度截断、列号转换、split index clamp，并补充 `str_display_width`。
2. `selection.rs` 的复制选中文本路径迁移到 `safe_char_slice` / `safe_str_slice_by_char`。
3. `output_area/mod.rs` 的 `screen_line_map.split_off` 迁移到 `clamp_split_index`。
4. `output_area/display.rs` 的宽度截断和列号转换委托给 `safe_text`。
5. `input_area.rs` 自动换行后缀提取改为 `safe_char_slice`。
6. 新增 `scripts/check-unsafe-text-ops.sh` 门禁，阻止 TUI 业务路径重新出现高风险切片/索引写法，当前 guard findings 已清零。
7. 补充 safe_text/display 相关边界测试，以及 markdown CJK link 渲染测试，覆盖 CJK 宽字符与安全索引场景。

**为什么要做（已踩过的坑）**：

| Bug | 路径 | 越界类型 |
|-----|------|----------|
| #4（archived）| Output area 渲染 | `screen_line_map` 索引越界 / CharIdx 运算溢出 / wrap 计算与 screen_line_map 不一致 |
| #5（archived）| 鼠标选中位置 | `screen_line_map` 滚动裁剪未同步 |
| #8（archived）| 字符串索引 | 字节/字符长度混淆 |
| #16（archived）| `/resume` 列表 CJK | `chars().nth(x_usize)` 用屏幕列号当字符索引 + `text.len()` 当显示宽度 |
| streaming.rs | thinking block | UTF-8 字节 boundary panic（4636/4636 修复） |
| #28（已代码修复，待确认归档）| 复制选中文本 | `chars[from..to]` 中 `from` 未做 `chars.len()` 裁剪；代码层已修复，仍在 `docs/bug/active.md` 等待用户确认归档 |

每次出 bug 各自修各自的，没有共享防御层 → 同样的"index 越界 / 字节-字符混淆 / CJK 宽字符当 1 列"模式会换个文件再出现。

**实际设计 / API**：

#### 1. 新增 `aemeath-cli/src/tui/safe_text.rs` 模块

实际 API（全部 panic-free，越界返回空切片、空字符串、`None` 或 clamp 后的位置）：

```rust
/// 按字符（不是字节）安全切片，from/to 都被 clamp 到 chars 长度，
/// 如 from >= to 视为空。
pub fn safe_char_slice(chars: &[char], from: usize, to: usize) -> &[char];

/// 按字符 index 安全取一个字符。
pub fn safe_char_at(s: &str, idx: usize) -> Option<char>;

/// 按字符 range 进行 clamp；空区间或反向区间返回 None。
pub fn clamp_char_range(from: usize, to: usize, chars_len: usize) -> Option<Range<usize>>;

/// 按字符范围安全切 `&str`，返回借用切片而非新分配 String。
pub fn safe_str_slice_by_char(s: &str, from: usize, to: usize) -> &str;

/// 按 unicode 显示宽度截断（CJK 占 2 列），返回 (substring, width_used)。
pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize);

/// 计算 unicode 显示宽度。
pub fn str_display_width(s: &str) -> usize;

/// 按 unicode 显示宽度从屏幕列号定位字符索引（鼠标点击/选中用）。
pub fn col_to_char_idx(s: &str, col: usize) -> usize;

/// clamp Vec::split_off 的 split index。
pub fn clamp_split_index(offset: usize, len: usize) -> usize;
```

实际实现偏差：
- `safe_str_slice_by_char` 返回 `&str`，不是早期草案里的 `String`。
- 未引入 `SafeChars` 包装类型；当前以函数式 helper 收口高风险操作。
- 未保留 `safe_byte_slice` / `safe_char_truncate` 命名；对应能力由 `safe_str_slice_by_char`、`truncate_unicode_width` 覆盖。

#### 2. 业务路径迁移范围

- `selection.rs::get_selected_text` → 改用 `safe_char_slice` / `safe_str_slice_by_char`。
- `output_area/mod.rs` 中 `screen_line_map.split_off` → 改用 `clamp_split_index`。
- `output_area/display.rs` 的宽度截断、显示宽度计算、列号转换 → 改用 `truncate_unicode_width` / `str_display_width` / `col_to_char_idx`。
- `input_area.rs` 自动换行后缀提取 → 改用 `safe_char_slice`。
- `markdown.rs` link 解析仍保留 `.get(byte_range)`：byte range 来自 `find()`，由 `get()` 验证 UTF-8 boundary；这些位置通过 `allow unsafe_text_op` 注释白名单，不代表全部直接切片都已消除。
- `streaming.rs` 继续使用 `aemeath-core/src/string_idx/` 的 `ByteIdx` / `StrSlice`，因为 thinking block 解析是字节级协议/标签扫描操作，不适合强行改成 TUI 字符切片 helper。

#### 3. caveat / 边界说明

- `safe_text` 是 TUI 字符索引 / 显示宽度安全层；`aemeath-core::string_idx` 是字节 / 字符强类型索引层。两者当前并存，未来可评估统一或抽象边界。
- `safe_text` 收口的是 TUI 业务路径里的高风险字符切片、显示宽度截断、列号换算和 split index；并非要求所有字节级解析都改成字符级 API。
- 验证 caveat：`cargo fmt --check` 仍有与 #23 无关的 `aemeath-core` 预存格式差异，因此本次文档 follow-up 以 `git diff --check` 作为必跑验证。

#### 4. lint / 测试门禁

- `safe_text` 模块每个函数至少 5 个测试：空输入、from=to、from>to、from>len、to>len、CJK 宽字符
- 加 clippy 自定义 lint 或 grep 检查脚本：`tui/` 目录下出现 `chars\[.+\.\..+\]` / `\.chars\(\)\.nth\(` / `s\[\d+\.\.\d+\]` 时 fail，强制走 `safe_text`
- CI 增加 panic stress test：构造各种边界输入（空字符串、纯 CJK、超长行、滚动裁剪后选中等）

#### 5. 实施分两阶段

**Phase 1（先止血）**：
- 修复当前 #28（最小修复 + 加 `if from > to { continue; }`）
- 新建 `safe_text.rs` 骨架，把 `safe_char_slice` / `clamp_char_range` 实现 + 测试
- `selection.rs` 迁移到新 API，作为示范

**Phase 2（全面收口）**：
- 把所有 TUI 路径的字符串索引/切片改为 `safe_text` API
- 加 grep 门禁脚本（CI 跑）
- 补 panic stress test

**为什么不简单"加 if 保护"了事**：
- 防御代码会被反复忘记加（#28 就是 #5 / #8 修过同类问题后又出现）
- 类型层面表达不出"这是字符 index 还是字节 index"，只能靠人脑追
- 单点保护无法覆盖未来新增的索引点

**涉及路径**：
- 新增：`aemeath-cli/src/tui/safe_text.rs`
- 重构：`aemeath-cli/src/tui/output_area/selection.rs` / `markdown.rs` / `streaming.rs` / `mod.rs`
- 重构：`aemeath-cli/src/tui/input_area.rs`
- 新增：`scripts/check-unsafe-text-ops.sh`（grep 门禁）
- CI：`.github/workflows/` 或本地 `Justfile` / `Makefile` 加调用

**关联**：
- Bug #4 / #5 / #8 / #16 / #28（全部是字符串/索引越界类）
- streaming.rs UTF-8 boundary 修复（已修，可作为 case 1 验证）

**开放问题**：
- `safe_text` 放在 `aemeath-cli/src/tui/` 还是提升到 `aemeath-core/src/utils/`（如果 core 也有类似需求）
- 是否引入 `unicode-segmentation` crate（按 grapheme cluster 而非 char 计算，更贴合"用户感知字符"）
- grep 门禁误报怎么处理（比如测试文件、`safe_text` 自己内部使用切片、经 `.get(byte_range)` 验证的字节范围）—— 可以加 `allow unsafe_text_op` 注释跳过

---

### #24 Spinner 下方 task list 限量显示（最多 7 条）

**目标**：当 task 数量较多（10+）时，spinner 下方的 task list 占据屏幕大量空间，把主对话/输出挤到看不见。改为按"前后文相关性"窗口化显示，固定上限 7 条左右，让用户能快速看到"刚做完什么、正在做什么、接下来做什么"，而不是被一长串 ☐ pending 淹没。

**当前现状**（`aemeath-cli/src/tui/app/mod.rs:639-672`）：
- `update_task_status()` 把当前 batch 内**所有**非 deleted 的 task 全部 push 到 `task_status_lines`
- 摘要行 `━━ Tasks: x/y ━━` + 每个 task 一行（`✓` / `■` / `□` + 编号 + 标题 + owner）
- 7 条 task → 占 8 行；20 条 task → 占 21 行；输出区域所剩无几

**预期窗口化策略**：

显示顺序（completed → in_progress → pending）：

```
━━ Tasks: 3/15 ━━              ← 摘要行始终反映全量
✓ #3 拆分 mod.rs                ← 上一条 completed，仅显示 1 条
■ #4 拆分 hook.rs               ← 所有 in_progress 全显示
■ #5 拆分 task.rs
□ #6 拆分 scheduler.rs           ← 后续 pending，按余量填充
□ #7 拆分 state.rs
□ #8 拆分 guidance.rs
… +7 more pending               ← 折叠提示
```

具体规则：
1. **摘要行保持全量**：`Tasks: x/y` 不受窗口限制
2. **窗口按优先级填充**（默认上限 7 条）：
   - 上一条 completed（最近完成的 1 条）
   - 所有 in_progress（一般 1~3 条）
   - 后续 pending 按 task id 升序填充剩余配额
3. **超出部分**：`… +N more pending` 单行折叠提示
4. **没有 in_progress 时**：第一条 pending 视为"接下来要做"，显示前 6 条 + `… +N more`
5. **全部 completed 时**：显示最后 5~7 条 completed
6. **空 task list**：不显示窗口

**配置项**：
```json
{
  "ui": {
    "task_list": {
      "max_lines": 7,
      "show_last_completed": 1,
      "fold_hint_format": "… +{n} more {status}"
    }
  }
}
```

**实施分解**：
1. `update_task_status()` 增加窗口化逻辑（分桶 → 按规则取窗口 + 折叠提示）
2. 拆出纯函数 `build_task_window(tasks, max_lines, last_completed_count) -> Vec<String>`，单独测试
3. 单元测试覆盖：0 / 1 / max / max+1 / 远超 max 各档；全 pending / 全 in_progress / 全 completed / 混合；in_progress 数量超过 max 时 pending 全部隐藏

**涉及路径**：
- `aemeath-cli/src/tui/app/mod.rs`（`update_task_status` 窗口化）
- 新增：`aemeath-cli/src/tui/app/task_window.rs`（纯函数 + 单元测试）
- `aemeath-core/src/config/`（`ui.task_list.max_lines` 等配置字段）

**关联**：
- Feature #18（task batch 机制）—— 在 batch 之上做窗口化，正交
- Feature #25（task 跨轮次生命周期）—— 限量解决"显示太多"，#25 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修复后窗口化逻辑会更频繁触发

**开放问题**：
- max 默认 7 是否合适？高分屏 vs 小屏权衡
- 折叠提示是否可点击展开？留作后续 polish
- 全部 completed 时显示 last 5 vs 折叠成 `Tasks: 15/15 ✓ all done`

---

### #25 Task list 跨轮次生命周期策略

**目标**：在同一 session 内，处理"上一轮的 task list 在新对话开始时还会显示"的问题。当前 Feature #18 的 batch 机制只是"新 turn 切到新 batch"，但没规定旧 batch 怎么收尾、怎么提示用户、何时归档。本 feature 补齐三种典型场景的明确策略。

**用户痛点**：「同一个 session 中，新的对话开始时还会显示上次的 task list」

具体场景：
- 上轮 task 全做完了 → 新对话开头还看到一长串 ✓，没价值还占地方
- 上轮 task 没做完用户主动问别的 → 旧 task 状态尴尬，是继续？是放弃？没出路
- 上轮 task 多轮没推进（用户跑题、agent 偏题）→ 沉默积压在 batch 里没人理

---

#### 场景 1：上一轮 task 全部完成

**触发**：上一 batch 内所有 task 都是 `Completed`（或 `Cancelled`），且用户输入新对话。

**策略**：
- 新 turn 开始时检测上一 batch 是否 100% 完成
- 是 → 自动隐藏旧 batch（保留在 TaskStore 历史中，可通过 `/task history` 回看）
- 显示一行 toast（1~2 秒）：`✓ 上一组 task 已完成（5/5）`
- 新 batch 在用户新 task 出现时才创建

#### 场景 2：上一轮 task 中断、用户开新话题

**触发**：上一 batch 内有 `InProgress` / `Pending` task，用户输入了一条**与未完成 task 主题不相关**的新消息。

**判断"主题不相关"**（启发式，不调 LLM）：
- 关键词重叠率低（task 标题与新消息分词后 cosine 相似度 < 0.2）
- 或：用户消息以 `/` 开头（slash 命令通常是控制流）
- 或：消息含明显切换语气（"先放一下"、"换个话题"、"另外"、"对了"等）

**策略**：弹 inline 提示（不阻塞输入）：
```
⚠ 上一组 task 还有 3 项未完成（#4 #5 #6），是否：
  [c] 继续上次任务   [p] 暂存稍后回来   [d] 丢弃这组任务
  （直接回车默认 [p] 暂存）
```

- `[c]` 继续：保留旧 batch 为当前 batch，新消息作为"补充指令"附加
- `[p]` 暂存：旧 batch 标记为 `paused`，从视图隐藏，可 `/task resume <batch_id>` 恢复
- `[d]` 丢弃：旧未完成全部 `Cancelled`，归档

#### 场景 3：旧 task 沉默积压

**触发**：某 batch 内有 `InProgress` / `Pending`，连续 N 轮（默认 3）用户对话没推进它（没 TaskUpdate 涉及它，没 tool call 修改了 task 涉及的文件等）。

**策略**：
```
ℹ 以下 task 已沉默 3 轮：
  ■ #4 拆分 hook.rs
  □ #5 拆分 task.rs
  仍要继续吗？回 /task keep 保留 / /task drop 丢弃 / /task pause 暂存
```

- 不打断当前对话，提示出现一次后不重复（直到再过 N 轮或用户回复）
- 提示文本不入 LLM context（仅 UI 可见，避免污染对话）

---

**配置项**：
```json
{
  "ui": {
    "task_lifecycle": {
      "auto_clear_completed_on_new_turn": true,
      "interrupt_prompt_enabled": true,
      "interrupt_default_action": "pause",
      "stale_remind_after_turns": 3,
      "stale_remind_repeat_interval": 5
    }
  }
}
```

**新增命令 / 状态**：
- `Task.batch_status`: `Active | Paused | Archived`
- `/task pause` —— 当前 batch → Paused
- `/task resume [batch_id]` —— 恢复指定 batch
- `/task keep` —— 沉默提示中确认保留
- `/task drop` —— 当前未完成全部 Cancelled
- `/task history` —— 列出本 session 内所有 batch

**实施分解**：
1. **TaskStore 扩展**：`batch_status` 字段、`Batch` 结构（id / created_at / last_active_turn / status）
2. **场景 1 检测**：`update_task_status()` 调用前 check 上一 batch → 全 completed 隐藏 + toast
3. **场景 2 启发式 + 提示 UI**：新增 `topic_relevance_check(prev_tasks, new_message)`，触发时 push `UiEvent::TaskInterruptPrompt`
4. **场景 3 沉默检测**：turn 结束 hook 中递增每个未完成 task 的 `silence_turns`；达阈值 push `UiEvent::TaskStaleReminder`
5. **命令实现**：`commands/task.rs` 增加 pause / resume / keep / drop / history

**涉及路径**：
- `aemeath-core/src/task.rs`（Batch 结构、batch_status、silence_turns）
- 新增：`aemeath-core/src/task/lifecycle.rs`（场景判定纯逻辑 + 单元测试）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 触发场景检测）
- `aemeath-cli/src/tui/app/update.rs`（处理 TaskInterruptPrompt / TaskStaleReminder UI 事件）
- 新增：`aemeath-core/src/command/commands/task.rs`（pause / resume / keep / drop / history）
- `aemeath-core/src/config/`（`ui.task_lifecycle` 配置）

**关联**：
- Feature #18（task batch 机制）—— 本 feature 在 batch 之上加生命周期状态
- Feature #24（task list 限量显示）—— 限量解决"显示太多"，本 feature 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修好后场景 1/3 才能准确触发

**开放问题**：
- 主题相关性判断用关键词重叠率够吗？误判率 vs 复杂度（要不要直接调 LLM？太重）
- 场景 2 提示 inline vs ask_user？倾向 inline，但要确认默认 `[p] pause` 不会让用户莫名其妙
- batch 归档保留多久？session 结束时持久化，session resume 时是否复活？
- `/task history` 输出格式：表格 vs 树形？

---

### #27 日志分化：input.log / output.log / tool.log

**目标**：在 Feature 27（日志系统职责分层）基础上，将 agent 交互日志从 `aemeath.log` 中分离为三个 JSON 格式文件，`aemeath.log` 收窄为应用诊断日志。

**文件布局**：
- `~/.aemeath/logs/input.log` — LLM 输入快照（新增 messages + system_blocks 摘要）
- `~/.aemeath/logs/output.log` — LLM 完整输出（content blocks + token 用量 + 耗时）
- `~/.aemeath/logs/tool.log` — 工具调用请求 + 结果（完整参数和输出）
- `~/.aemeath/logs/aemeath.log` — 应用诊断日志（MCP、hook、session、技能、UI 调试）
- `~/.aemeath/logs/panic.log` — panic 信息

**JSON 统一格式**：
```json
{"ts":"2026-05-09T10:30:00+08:00","session":"abc123","turn":3,"role":"searcher","model":"gpt-5.5","type":"input","data":{...}}
```
- `role` 为 agent role（主 agent 固定 `"default"`），`type` 为 `"input"` / `"output"` / `"tool_call"` / `"tool_result"`

**移除的旧代码**：`log_agent_loop_event()`、`log_llm_request_messages()`、`log_tool_result_event()`

**新增组件**：`JsonLogger`（`aemeath-core/src/logging.rs`），直接 `BufWriter<File>` 写入，不经 `env_logger`

**配置**：`logging.logs_dir`（默认 `~/.aemeath/logs/`）、`logging.role_logs_enabled`（默认 `true`）

**当前状态**：修复中；已修复 TUI `input.log` 只写 injected messages 导致用户输入缺失的问题，并补齐 REPL 的 input/output/tool 分化日志写入。验证：`cargo test -p aemeath-cli test_logged_input_messages`、`cargo test -p aemeath-core test_json_logger_log_input_happy_path_writes_user_message`、`cargo check -p aemeath-cli` 通过（仅既有 `Cmd::Batch` dead_code warning）。

详见 [design spec](../superpowers/specs/2026-05-09-log-split-design.md)

**实施完成（2026-05-11）**：
- `aemeath-core/src/logging.rs`：新增 `LogFile::Input/Output/Tool`、`JsonLogger` struct（含 `log_input/log_output/log_tool_call/log_tool_result`）、`logs_base_dir()`、日志轮转逻辑
- `aemeath-core/src/config/logging.rs`：新增 `logs_dir`、`role_logs_enabled` 配置字段
- `aemeath-cli/src/main.rs`：全局 `JSON_LOGGER` OnceLock + `get_json_logger()` 访问器，在 `init_logging()` 中初始化
- `aemeath-cli/src/tui/app/stream.rs`：`log_agent_loop_event` / `log_llm_request_messages` 内部增加 JsonLogger 写入（input/output/tool call/tool result）；`log_tool_result_event` 委托到 `log_agent_loop_event`
- `aemeath-cli/src/agent_runner.rs`：子 agent 的 `log_request_messages` 闭包 + LLM 响应处 + tool call 批量处 + tool result 处均已接入 JsonLogger
- 测试：`aemeath-core` 368 测试全部通过

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

### #4 AskUserQuestion TUI 美化

**目标**：当 LLM 调用 AskUserQuestion tool call 时，TUI 的等待输入界面要清楚说明下一步操作，并避免提示文字挤占过多 output area。

**当前状态**：待实施。基础问答链路已存在（`UiEvent::AskUser` + `ask_user_reply_tx`），本轮聚焦交互文案和选择模式：

- AskUserQuestion 等待输入时，在 output area 提示「请在下方输入区域输入」。
- 操作指引放在提示文字末尾：`[Enter] 确认 / [Esc] 取消 / [Tab] 切换选项`，不再单独占一行。
- 纯选择模式（仅 options、无 free_input）不要求用户手动输入选项文本，改为上下键高亮选项 + Enter 确认。
- 多选/自由输入保持现有能力，但提示文案需要与实际可用快捷键一致。

**涉及路径**：
- `aemeath-cli/src/tui/app/update/ui_event.rs`（`UiEvent::AskUser` 状态切换与等待提示）
- `aemeath-cli/src/tui/app/update/ask_user_key.rs`（选项导航、确认/取消、自由输入）
- `aemeath-cli/src/tui/output_area/content.rs`（AskUserQuestion 提示与选项渲染）
- `aemeath-cli/src/tui/output_area/types.rs`（AskUserQuestion 行样式）

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

#### 完成度评估（2026-05-09）：~75%

**已实现**：

| 组件 | 位置 |
|------|------|
| 存储层 `MemoryStore`（CRUD + 搜索 + 去重 + 归档） | `memory/store.rs` |
| `MemoryEntry` 结构（id/layer/category/tags/pin/ttl/outdated/access_count） | `memory/entry.rs` |
| 分类：Fact/Decision/Preference/Pattern/Pitfall | `memory/entry.rs` |
| 两层存储：Global + Project | `memory/store.rs` |
| 去重（Jaccard 相似度） | `memory/dedup.rs` |
| 评分（injection_score + eviction_score） | `memory/scoring.rs` |
| System Prompt 注入（`top_for_inject` → `# Project Memory`） | `prompt.rs:223-251` |
| `/memory` 命令（add/delete/pin/search/compact/stats） | `command/commands/memory.rs` |
| `MemoryTool`（LLM 可通过 tool call 操作 memory） | `memory_tool.rs` |
| `SessionReminders`（会话级提醒） | `memory/session_reminder.rs` |
| 配置（`MemoryConfig` + `ReflectionConfig`） | `config/memory.rs` |

**未实现**：

| 需求 | 说明 |
|------|------|
| `auto_summary_on_session_end` | 配置项存在但无调用代码，SessionEnd 时没有 LLM 总结写入 memory |
| `ReflectionGenerated` Hook 事件 | spec 中提到的 hook 事件不在 `HookEvent` 枚举中 |
| PostCompact 提取记忆 | PostCompact hook 只记录日志，没有提取被压缩内容到 memory |
| 淘汰策略定时触发 | 有 `compact()` + `eviction_candidates()`，但无定时触发逻辑，`archive_after_days` 配置项不在 `MemoryConfig` 中 |
| LLM 合并相近记忆 | spec 中"超过 100 条时用 LLM 合并"未落地 |
| SessionReminders 持久化 | `SessionReminders` 仅内存态，不写入文件，会话结束即丢失 |

---

### #9 反思系统（初版设计）

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

**开放问题**：
- 反思是否消耗当前 session 的 model 调用，还是用独立的轻量 model（成本权衡）
- 反思失败（如 LLM 返回空）时是否静默丢弃 vs 提示用户
- Memory 容量上限策略：何时压缩 / 淘汰旧反思

---

### #9 反思系统（实施版）

#### 完成度评估（2026-05-09）：~45%

**已实现**：

| 组件 | 位置 |
|------|------|
| `ReflectionEngine`（解析 JSON、格式化输出） | `reflection/mod.rs` |
| Prompt 模板（偏差检测 + 建议记忆 + 过时记忆） | `reflection/prompt.rs` |
| `ReflectionOutput` 结构 | `reflection/mod.rs` |
| `run_reflection()` 共享函数（TUI + REPL 复用） | `cli/src/reflection.rs` |
| 定时触发（每 N turns，LLM 调用） | `stream.rs:415` |
| Lightweight fallback（LLM 失败时的轻量反思） | `reflection.rs:131` |
| `/reflect` 命令（手动触发 + apply 子命令） | `command/commands/reflect.rs` |
| `apply_outdated()` 标记过时记忆 | `reflection/mod.rs:49` |
| `recent_messages_summary()` 提取对话摘要 | `reflection/mod.rs:122` |
| 配置（`ReflectionConfig`） | `config/memory.rs` |

**未实现（核心缺口——反思结果没有闭环）**：

| 需求 | 说明 |
|------|------|
| **建议记忆自动写入 MemoryStore** | `suggested_memories` 只展示、从未写入 store。`auto_apply_suggestions` 配置项存在但无使用代码 |
| **过时记忆自动标记** | `apply_outdated()` 已实现但从未被调用 |
| 连续工具失败触发反思 | spec P1 |
| SessionEnd 反思 | agent loop 结束时未触发反思 |
| SubagentStop 反思 | spec P2 |
| 用户中断反思 | spec P2 |
| 错误恢复后反思 | spec P2 |

**行为与 spec 不一致**：

| spec 要求 | 实际行为 |
|-----------|----------|
| 反思结果静默写入 MemoryStore，不显示在对话中 | 当前通过 `UiEvent::SystemMessage` **展示给用户** |
| 使用独立轻量模型（成本优化） | `ReflectionConfig.model` 字段存在，但 `run_reflection()` 使用传入的主模型 client |
| 后台异步执行（不阻塞主循环） | `run_reflection()` 是 `.await` 的，**阻塞** agent loop |

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

### #30 Agent loop 收尾工作

**实施完成（2026-05-11）**：

**新增类型**（`aemeath-cli/src/agent_runner.rs`）：
- `AgentRunStatus`：`Completed | Cancelled | TimedOut | ApiError(String) | MaxTurns`
- `AgentRunOutcome`：`{ status, turns, duration, role, model }`
- `log_agent_outcome()`：统一写 `[agent_loop_finished]` 结构化日志
- `finalize_and_return!` 宏：子 agent 5 个退出路径统一收口
- `finalize_sub_agent()`：恢复 client 设置 + SubagentStop hook + 日志

**主 loop 收口**（`aemeath-cli/src/tui/app/stream.rs`）：
- 所有退出路径改为 `break` + `outcome`，替代 `return`
- `finalize_main_loop()`：根据 outcome.status 统一触发 Stop/StopFailure hook、日志、task batch 归档
- 打断、API 错误路径现在也走 `agent_loop_finished` 日志

**Task reminder 注入优化**（`aemeath-cli/src/task_reminder.rs`）：
- 首次注入豁免间隔检查，对话第 1 个 turn 即注入 reminder
- 后续每 5 turn 保持原规则

**Task batch 自动归档**：
- Completed/MaxTurns 退出时检查活跃 batch 是否全部完成
- 全部完成 → 归档（`BatchStatus::Archived`），下次对话不再提醒

**已实现 vs 原 P0 范围**：
1. ✅ `AgentRunStatus` / `AgentRunOutcome`
2. ✅ 统一 finalize 函数（main loop + sub agent）
3. ✅ 恢复 client 设置、SubagentStop hook、结构化日志摘要
4. ✅ 保持对外行为不变
5. ✅ Task batch 归档逻辑、首次 reminder 注入（超出原 P0）

**未做（后续 P1/P2）**：
- session 持久化接入 finalize
- tool 资源释放/取消 token 清理
- Task `Interrupted` 状态（当前 Cancelled 时 task 保持原状）
- Ctrl+C 外层处理（仍在 update.rs，未收口到 finalize）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（AgentRunStatus/Outcome、log_agent_outcome、finalize_sub_agent、宏）
- `aemeath-cli/src/tui/app/stream.rs`（finalize_main_loop、退出路径改造）
- `aemeath-cli/src/task_reminder.rs`（首次注入豁免）
- `docs/superpowers/specs/2026-05-11-agent-loop-finalize-design.md`（设计文档）

**测试**：cargo test 506 passed, 0 failed

---

### #31 TUI 架构守卫脚本（TEA 纯度 + 400 行限制）

**目标**：通过两个 lint 脚本 + pre-commit/pre-push hook，在 CI 本地阶段就拦截两类架构偏移：

1. **TUI 层偏离 TEA 架构**：TUI 的 `update()` 应只返回 `Cmd` 描述，由 runtime 执行副作用。禁止在 update 路径中直接 `tokio::spawn`、写文件、发网络请求、操作 clipboard/image 等。
2. **文件超过 400 行**：CLAUDE.md 规定单个 `.rs` 文件不超过 400 行（含测试代码），超出时应拆分职责。

#### 脚本 1：`scripts/check-tea-architecture.sh`

**检查范围**：`aemeath-cli/src/tui/` 目录下所有 `.rs` 文件。

**检查规则**：

| 规则 | 模式 | 说明 |
|------|------|------|
| 禁止直接 tokio::spawn | `tokio::spawn` | 应通过 `Cmd::SpawnTask` 等 Cmd 描述 |
| 禁止直接文件写入 | `std::fs::write\|File::create\|OpenOptions::new().write` | 应通过 `Cmd::WriteFile` |
| 禁止直接网络请求 | `reqwest::Client\|HttpConnector` | 应通过 `Cmd::HttpRequest` |
| 禁止直接 clipboard 操作 | `arboard::Clipboard\|set_clipboard` | 应通过 `Cmd::SetClipboard` |
| 禁止直接 image 操作 | `image::open\|image::load_from` | 应通过 `Cmd` 描述 |

**白名单机制**：行尾注释 `// allow-tea: <reason>` 可豁免（如 `Cmd` runtime 执行层本身需要调用这些 API）。

**退出码**：发现违规 → exit 1，输出违规文件和行号。

#### 脚本 2：`scripts/check-file-lines.sh`

**检查范围**：所有 crate 下的 `.rs` 文件（`aemeath-core/`、`aemeath-cli/`、`aemeath-llm/`、`aemeath-tools/`）。

**检查规则**：

| 规则 | 说明 |
|------|------|
| 单文件 ≤ 400 行 | `wc -l < file` 超过 400 行即报错 |
| 跳过生成文件 | 排除 `target/`、`.worktrees/` |

**退出码**：发现超限 → exit 1，输出超限文件和行数。

#### Hook 集成

- **pre-commit**（`.git/hooks/pre-commit` 或 `lefthook`/`husky`）：运行两个脚本
- **CI**（可选）：`.github/workflows/` 中同步调用
- **手动**：`./scripts/check-tea-architecture.sh && ./scripts/check-file-lines.sh`

#### 实施分解

1. 创建 `scripts/check-tea-architecture.sh`
2. 创建 `scripts/check-file-lines.sh`
3. 配置 git hook（推荐 lefthook 或手动 symlink）
4. 运行一次，修复现有违规（如有的话先记录到白名单）
5. 文档更新：CLAUDE.md 验证门禁段加入两个脚本调用

#### 涉及路径

- 新增：`scripts/check-tea-architecture.sh`
- 新增：`scripts/check-file-lines.sh`
- 修改：CLAUDE.md（验证门禁段）
- 修改：git hook 配置

#### 关联

- CLAUDE.md 编码规范（400 行限制、update() 副作用禁止）
- Feature #23（safe_text lint 脚本）—— 已有 `check-unsafe-text-ops.sh` 可参考

#### 开放问题

- TEA 架构白名单粒度：按行还是按函数？按行更精确但维护成本高
- 400 行限制是否包含空行和注释？建议包含（`wc -l` 天然包含）
- 是否需要自动修复能力（如 `--fix` 自动拆分超长文件）？建议不做，手动拆分更可控

---

### #32 TUI 选中和复制逻辑统一

**目标**：将 output area、input area、status line 三处的选中（selection）和复制（copy）逻辑统一为可复用组件或 trait，消除行为差异和重复代码。

**现状问题**：
- **output area**：有完整的 selection state（起止坐标）、screen_line_map 映射、渲染高亮、Ctrl+C 复制
- **input area**：有独立的 selection 逻辑（基于编辑器光标位置），API 和数据结构与 output area 不同
- **status line**：基本无可选中/复制能力（task list 行即 Bug #33）
- 三处对鼠标事件（按下、拖拽、释放）的处理方式不一致
- 复制到 clipboard 的调用路径各不相同

**统一方案**：
1. 抽取 `SelectableRegion` trait 或 struct，封装：
   - selection state（start/end 坐标）
   - screen_line_map（屏幕行 → 文本行映射）
   - 鼠标事件处理（按下→开始选择、拖拽→更新范围、释放→结束选择）
   - 渲染高亮（选中区域反色）
   - 复制功能（提取选中文本、写入 clipboard）
2. output area / input area / status line 分别实现或持有该组件
3. 统一 clipboard 调用（通过 `Cmd::SetClipboard`，不直接调用 arboard）

**实施分解**：
1. 审计现有三处 selection/copy 代码，提取共性
2. 设计 `SelectableRegion` API
3. 先在 output area 上重构验证
4. 逐步迁移 input area、status line
5. 修复 Bug #33（task list 无法选中）作为验证

**关联**：
- Bug #33（spinner 下方 task list 无法选中和复制）
- CLAUDE.md 编码规范（DRY 原则）

**开放问题**：
- input area 的 selection 与编辑器光标耦合较深，统一后是否需要保留编辑器特有的 selection 行为？
- status line 内容多为动态生成（spinner、task list），selection 的文本提取需要统一的数据源接口

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

### #35 Diff 渲染中 add 行语法高亮

**目标**：LLM 输出的 unified diff（```diff 代码块）中，`+` 开头的 add 行按目标文件语言做语法高亮，让用户更直观地看到新增代码的结构。

**当前现状**：
- TUI 的 markdown 渲染器已支持 ```diff 代码块的基本着色（`+` 行绿色、`-` 行红色）
- 但 add 行内部没有按语言做语法高亮，所有 add 内容统一绿色文本，难以区分关键字、字符串、注释等
- 项目中目前无语法高亮基础设施（无 tree-sitter / syntect 等依赖）

**预期效果**：
```
-旧的纯绿色 add 行（无结构高亮）
+fn main() {     ← 关键字/函数名/括号各按语言规则着色
+    let x = 1;  ← 类型/变量/数值有区分
+}
```

**关键技术选型**：

| 方案 | 优点 | 缺点 |
|------|------|------|
| **tree-sitter** | 精确解析、增量更新、多语言 | 编译慢、需为每种语言生成 .so |
| **syntect**（TextMate grammar） | 纯 Rust、开箱即用、主题丰富 | 解析精度略低于 tree-sitter |
| **bat / ora** 命令行工具调用 | 零集成成本 | 依赖外部工具、每次高亮 fork 进程 |

推荐 **syntect**，纯 Rust、无外部依赖、与 ratatui 渲染管线契合度高。

**实施分解**：

1. **diff 块检测与语言推断**：markdown 渲染器识别 ```diff 块时，从 diff header（`--- a/foo.rs` / `+++ b/foo.rs`）提取文件扩展名，推断目标语言
2. **语法高亮层**：引入 `syntect` crate，封装 `highlight_line(line, language, theme)` 函数，返回 ratatui 可用的 `Vec<(Color, &str)>` 分段
3. **渲染集成**：diff 渲染器对 `+` 行去掉前导 `+` 后调用高亮层，将结果拼回绿色前缀 + 高亮内容
4. **降级策略**：语言推断失败时回退到当前纯绿色渲染
5. **主题适配**：syntect 主题颜色映射到终端 256 色或真彩色，兼顾暗色/亮色终端

**涉及路径**：
- `aemeath-cli/src/tui/output_area/markdown.rs`（diff 块渲染）
- 新增：`aemeath-cli/src/tui/syntax.rs`（语法高亮封装）
- `aemeath-cli/Cargo.toml`（新增 `syntect` 依赖）

**关联**：
- TUI markdown 渲染管线
- Feature #23（safe_text）—— 高亮后的分段渲染需走安全索引

**开放问题**：
- `-` 行（删除内容）是否也做语法高亮？建议不做，灰色/红色保留即可，减少视觉噪声
- diff 中混合多文件时语言推断策略：按每个 `---`/`+++` 块切换语言
- 性能：syntect 首次加载 grammar 集合有延迟（~50ms），是否需要 lazy init 或预加载常用语言子集
- 终端颜色支持：是否检测终端真彩色能力降级到 256 色
