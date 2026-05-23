# Feature #44：Commit Style Context 与 AI 协作者 trailer

## 背景

不同项目的 commit message 风格不同。用户希望 Aemeath 在需要创建 git commit 时，先参考当前项目历史中带 `Co-Authored-By` 的提交风格，再生成符合项目习惯的 commit message。

经过讨论后，本 feature 不在 session 初始化时执行 `git log` 或提前分析历史提交，避免增加启动开销和无关 prompt。系统只在 system prompt 中注入一条稳定规则：当需要创建 commit 时，LLM 必须先分析当前仓库的 Commit Style Context，再撰写 commit message。

## 目标

1. 在 system prompt 中加入 commit message guidance。
2. guidance 要求 LLM 在 commit 前主动分析当前仓库历史 commit 风格，优先查看包含 `Co-Authored-By` 的提交。
3. guidance 提供当前 AI 协作者 trailer 模板，且 trailer 包含当前 provider/model 信息。
4. 不在 session 初始阶段自动执行 git log，不提前生成历史摘要。
5. 不伪造人类协作者；AI 协作者 trailer 使用 Aemeath 固定身份。

## 非目标

1. 不实现项目级 commit 风格缓存。
2. 不在每轮对话重复注入历史 commit 摘要。
3. 不强制每个 commit 都必须带 trailer；是否添加由 LLM 结合历史风格和用户意图决定。
4. 不自动改写用户手写的 commit message。

## Prompt 规则

system prompt 应新增类似规则：

```text
Commit message guidance:
- Before creating a git commit, inspect this repository's recent commit history.
- Prefer sampling commits that contain `Co-Authored-By` to infer commit style and trailer conventions.
- Analyze title format, type/scope usage, body style, language, footer/trailer style, and whether AI co-author trailers are commonly used.
- Keep the final commit message consistent with this repository's existing style.
- Do not invent human co-authors.
- When an AI co-author trailer is appropriate, use exactly:
  Co-Authored-By: Aemeath (<provider>/<model>) <github:rushsinging/aemeath>
```

其中 `<provider>` 与 `<model>` 来自当前 LLM client：

- `LlmClient::provider_name()`
- `LlmClient::model_name()`

示例：

```text
Co-Authored-By: Aemeath (zhipu/glm-5.1) <github:rushsinging/aemeath>
```

## 行为约束

1. LLM 准备提交前应主动使用 git 历史分析命令，例如：
   - `git log --format=%B --grep='Co-Authored-By' -n <N>`
   - 必要时 fallback 到近期普通提交：`git log --format=%B -n <N>`
2. 采样必须限制数量和输出长度，避免把大量历史提交塞入上下文。
3. LLM 应提炼风格后再生成 commit message，而不是直接复制历史提交。
4. AI trailer 使用固定身份 `Aemeath`，并把当前 provider/model 放入 name 部分。
5. 不应新增其他未经用户确认的人类 `Co-Authored-By` trailer。
6. 如果项目历史没有 co-author 风格，LLM 可以不添加 trailer，或在用户明确要求时添加 Aemeath trailer。

## 实现建议

### PromptContext 扩展

当前 system prompt 构建入口位于 `cli/src/prompt.rs`。建议扩展 prompt 构造上下文，让其能拿到当前 provider/model：

```text
PromptContext
- cwd
- provider_name: Option<String>
- model_name: Option<String>
```

TUI / REPL 调用 `build_system_prompt_parts` 时，使用已持有的 `Arc<LlmClient>` 传入：

```text
client.provider_name()
client.model_name()
```

### Commit guidance 构造

新增一个小函数集中生成 commit guidance，避免在 TUI / REPL 重复字符串：

```text
build_commit_guidance(provider, model) -> String
```

当 provider/model 不可用时，fallback：

```text
Co-Authored-By: Aemeath (unknown/unknown) <github:rushsinging/aemeath>
```

或省略括号中的模型信息。建议 P0 使用 `unknown/unknown`，保证 prompt 结构稳定。

### 注入位置

将 commit guidance 放在 dynamic system prompt 中，与当前 cwd / date 等运行上下文相邻。它是规则，不是历史摘要，因此不需要每轮重新分析。

## 验收标准

1. system prompt 中包含 Commit message guidance。
2. guidance 明确要求 commit 前分析当前仓库历史，优先采样带 `Co-Authored-By` 的提交。
3. guidance 中 AI trailer 包含当前 provider/model：
   `Co-Authored-By: Aemeath (<provider>/<model>) <github:rushsinging/aemeath>`。
4. session 初始化不执行 git log，不生成历史摘要。
5. TUI 与 REPL 使用同一套 guidance 构造逻辑。
6. provider/model 不可用时有稳定 fallback。
7. `docs/feature/active.md` 同步更新 #44 状态和目标描述。
