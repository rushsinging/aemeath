# Prompt / Guidance

**Scope**：`agent/features/prompt/**`——Guidance 系统、系统提示构建、上下文注入。
**主触发**：改 `agent/features/prompt/**`。
**次触发**：改 provider 默认 model（影响 guidance 前缀匹配），或改系统提示注入。

## 3.7.1. Guidance 系统

- 实现：`agent/features/prompt/src/business/guidance/`（+ `guidance.rs`）。
- Guidance 文件存放在 `~/.agents/guidance/`：
  - `_default.md` — 所有模型通用。
  - `{prefix}.md` — 按 model id 前缀匹配（**最长匹配优先**）。
  - `_reasoning.md` — reasoning 开启时附加。
- 首次运行自动生成默认文件，**不覆盖**用户编辑。
- 改 provider 默认 model 时注意：model id 变化会影响 `{prefix}.md` 的命中，需确认对应 guidance 仍匹配。

## 3.7.2. 多语言一致性（Config.language）

所有注入 LLM context 的文本 **MUST** 按 `Config.language`（`"en"` / `"zh"`）提供对应语言版本。**NEVER** 在同一 system prompt 中混合中英文（结构性标签如 XML tag name 除外）。

### 3.7.2.1. 当前覆盖状态

| 注入路径 | 状态 | 位置 |
|---|---|---|
| Guidance 文件（`_default.md` 等） | ✅ 已双语（`DEFAULT_FILES_EN` / `DEFAULT_FILES_ZH`） | `constants.rs` |
| `UNIVERSAL_EXECUTION_DISCIPLINE` | ✅ 已双语（`universal_execution_discipline(lang)`） | `constants.rs` |
| task reminder 模板 | ✅ 已双语（`build_reminder(lang)`） | `task_reminder.rs` |
| `static_system_prompt_for()` | ✅ 已双语（`STATIC_SYSTEM_PROMPT_EN` / `_ZH`） | `prompt_build.rs` |
| `build_commit_guidance()` | ✅ 已双语（match lang 模板） | `prompt_build.rs` |
| `currentDate` 段 | ✅ 已双语 | `prompt_build.rs` |
| git context 标签 | ✅ 已双语（`GitContextLabels`） | `git_context.rs` |
| `# Available Skills` / `# Available Agent Roles` | ✅ 已双语 | `prompt_build_ext.rs` |
| claudeMd system-reminder 包裹文本 | ✅ 已双语 | `loop_runner.rs` |
| `Tool {} denied` / `Cancelled by user` / `Blocked by PreToolUse hook` | ✅ 已双语 | `tools.rs` / `non_agent.rs` |
| guidance 重载提示 | ✅ 已双语 | `loop_runner.rs` |
| Stop hook 反馈 | ✅ 已双语（`HookFeedbackLabels`） | `finalize.rs` |

### 3.7.2.2. 改造原则

1. **MUST** 在注入文本的函数签名中传入 `language: &str`（或 `Config`），按值选择对应语言版本。
2. **MUST** 为每个文本块定义 `const XXX_EN: &str` 和 `const XXX_ZH: &str`，再通过 `fn xxx(lang) -> &'static str` 选择——**NEVER** 在调用点内联 match。
3. **SHOULD** 优先双语化直接面向 LLM 行为指令的文本（system prompt、task reminder），其次处理反馈性文本（hook 反馈、tool denied）。
4. **MAY** 对纯结构性标签（`<system-reminder>` XML tag name、JSON key name）保持英文不翻译。
