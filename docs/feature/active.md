# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 4 | AskUserQuestion TUI 美化 | - | 待实施 | 未确认 | AskUserQuestion 向用户确认时，TUI 界面需要美化 |
| 8 | Memory 系统 | - | 重新设计中 | 未确认 | 跨会话持久化记忆，记忆作为一等公民，LLM 自主管理 + Hook 兜底。详见 [spec](specs/008-memory-system.md) |
| 9 | 反思系统 | - | 重新设计中 | 未确认 | 关键节点自动反思，发现偏差并提炼经验写入 Memory（依赖 #8）。详见 [spec](specs/009-reflection-system.md) |
| 17 | Skill 延迟加载 + 命名空间前缀 | - | ✅ 已完成 | 未确认 | 启动只读 frontmatter 不读全文，Skill 工具调用时按需加载；skill 包自动加 `plugin_name:` 前缀；HookJsonOutput camelCase 反序列化修复 |
| 18 | Task list 跨轮次 batch 机制 | - | ✅ 已完成 | 未确认 | Task 跟随 session 持久化，不再每次用户消息清空；按 batch 分组显示，新 turn 自动切换到新 batch，旧 batch 隐藏；已完成 task 在当前 batch 内继续显示 |
| 21 | TUI 优化 Agent 调用输出展示 | - | ✅ 已完成 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
| 23 | TUI 字符串/切片安全索引收口 | 高 | 待确认 | 未确认 | 把"按字符索引/切片"等易越界操作收口到 `safe_text` 工具模块，提供 `safe_char_slice`、`safe_str_slice_by_char`、`clamp_char_range`、`truncate_unicode_width`、`col_to_char_idx`、`safe_char_at`、`clamp_split_index`、`str_display_width` 等实际 API，禁止业务路径直接 `chars[from..to]` / `s[i..j]`。配合 lint 规则与单元测试覆盖边界，根治 Bug #4 / #8 / #28 类 panic |
| 24 | Spinner 下方 task list 限量显示（最多 7 条） | 中 | ✅ 已完成 | 未确认 | task 多时显示过长挤占主输出。改为窗口化显示：上一条 completed + 所有 in_progress + 后续 pending，总数封顶 7 条；其余以 `… +N more` 折行提示。摘要行 `Tasks: x/y` 仍反映全量进度 |
| 25 | Task list 跨轮次生命周期策略 | 中 | ✅ 已完成 | 未确认 | 同 session 新对话开始时仍显示上次的 task list。补齐三种场景策略：① 全部完成时自动清屏归档；② 中断未完成时提示用户「继续 / 暂存 / 丢弃」；③ 多轮未推进的旧 task 自动提醒确认是否继续 |

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

### #4 AskUserQuestion TUI 美化

**目标**：当 LLM 调用 AskUserQuestion tool call 时，TUI 中的确认界面需要美化，提升可读性和交互体验。

**当前状态**：基础功能已实现（`UiEvent::AskUser` + `update.rs` 中 `ask_user_reply_tx` 机制），但显示为普通 system message + 纯文本选项，缺乏视觉层次。

**待改进**：
- 问题文本高亮/醒目样式
- 选项列表带序号和视觉区分
- 输入提示区域样式优化

**涉及路径**：`aemeath-cli/src/tui/app/update.rs`（`UiEvent::AskUser` 处理）、`aemeath-cli/src/tui/output_area/`（渲染样式）

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
