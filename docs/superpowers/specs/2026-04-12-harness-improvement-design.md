# aemeath Harness 改进设计

> 日期：2026-04-12
> 状态：已确认
> 方案：配置驱动式（方案 B）

## 背景

基于 Hermes Agent 与 OpenClaw 的对比分析，aemeath 在以下 5 个方向存在改进空间。目标是通过更精细的提示工程和上下文管理，提升模型在 aemeath 中的表现——特别是国产模型（GLM-5.1、MiniMax-M2.7 等）的已知失败模式：

- **光说不练**：描述计划但不调用工具
- **子 Agent 拆解过粗**：派发任务颗粒度偏大
- **凭记忆瞎编**：不读文件直接猜测内容

## 总体架构

### System Prompt 组装顺序

```
1. 原有 static_part（身份、工具指导、风格、环境等）
2. [NEW] 通用执行纪律（所有模型，不可覆盖）
3. [NEW] 模型家族指导（按 provider 自动匹配，可被 config guidance 覆盖）
4. skills 列表
→ 合并为 SystemBlock::cached(...)

5. 原有 dynamic_part（日期、git 状态）
→ SystemBlock::dynamic(...)
```

### 核心原则

- 内置默认保证开箱即用
- 配置文件可覆盖模型家族指导，无需重编译
- 通用执行纪律始终注入，不可覆盖
- 所有模块独立，可按顺序逐个实现

---

## 模块 1：模型特定行为纠正

### 1.1 通用执行纪律

作为 Rust 常量内置，注入所有模型的 system prompt。包含 6 个子模块：

```text
# 执行纪律

<tool_persistence>
持续调用工具直到任务完成且结果已验证。不要停下来总结你做了什么。
用户要的是结果，不是过程描述。
</tool_persistence>

<mandatory_tool_use>
以下场景必须使用工具，禁止凭记忆或推理回答：
- 文件内容/结构 → 使用 Read、Glob、Grep
- 代码修改 → 先 Read 再 Edit，不要猜测文件内容
- 系统状态/命令输出 → 使用 Bash
- 数学计算 → 使用 Bash
</mandatory_tool_use>

<act_dont_describe>
说要做的事必须立即调用工具执行。
禁止以"我将..."、"让我..."结尾而不附带工具调用。
每个响应必须包含工具调用或最终结果。
</act_dont_describe>

<agent_decomposition>
派发子 Agent 时，每个子 Agent 只负责一个具体、可验证的小任务。
错误示范："分析整个模块的架构"
正确示范："读取 src/config.rs 的 ModelEntryConfig 结构体，列出所有字段及类型"
</agent_decomposition>

<prerequisite_checks>
执行修改前必须先验证前提条件：
- 修改文件前 → Read 确认当前内容
- 运行命令前 → 确认依赖存在（如 package.json、Cargo.toml）
- 调用 API 前 → 确认配置和认证信息
</prerequisite_checks>

<verification>
任务完成后必须验证：
- 代码修改 → 编译/运行验证无报错
- 文件创建 → Glob 或 Read 确认已写入
- 配置变更 → 实际加载测试
不要声称"已完成"而未经验证。
</verification>
```

### 1.2 模型家族指导（内置默认）

按 provider name 自动匹配，存储为 Rust 常量：

| provider 匹配 | 默认指导要点 |
|---|---|
| `zhipu`、`packyapi`（GLM 系列） | 不要用中文复述工具输出；JSON 参数必须严格格式 |
| `minimax` | thinking 内容已单独展示，正文直接输出结论 |
| `ollama` | 本地模型，响应可能慢，避免超大工具输出 |
| `anthropic` | 无额外指导 |
| 其他 | 仅注入通用执行纪律 |

### 1.3 配置覆盖机制

在 `config.json` 的 `models` 层级新增 `guidance` 字段，支持通配符模糊匹配：

```json
{
  "models": {
    "guidance": {
      "zhipu/*": "~/.aemeath/guidance/glm.md",
      "minimax/*": "~/.aemeath/guidance/minimax.md",
      "*/glm-*": "~/.aemeath/guidance/glm.md"
    }
  }
}
```

**匹配目标**：`provider/model_id`（如 `zhipu/glm-5.1`）

**通配符规则**：
- `*` 匹配任意字符序列
- `zhipu/*` → 匹配 zhipu 下所有模型
- `*/glm-*` → 匹配任意 provider 下以 glm- 开头的模型
- `minimax/MiniMax-M2.7` → 精确匹配

**优先级**：精确匹配 > 通配符匹配（通配符越少越优先） > 内置默认

**加载流程**：
1. 构造 `provider/model_id`
2. 在 `guidance` map 中查找最佳匹配
3. 命中 → 读取文件内容，**替换**内置模型家族指导
4. 未命中 → 使用内置默认
5. 通用执行纪律始终注入，不受影响

### 1.4 代码改动

- `aemeath-core/src/config.rs`：`ModelsConfig` 新增 `guidance: HashMap<String, String>` 字段；新增 `find_guidance(provider: &str, model_id: &str) -> Option<String>` 方法
- `aemeath-cli/src/main.rs`：system prompt 组装时，在 static_part 之后追加通用执行纪律 + 模型家族指导

---

## 模块 2：工具使用强制引导

已合并到模块 1 的通用执行纪律中（`prerequisite_checks` 和 `verification` 子模块）。无需独立实现。

---

## 模块 3：结构化上下文压缩

### 3.1 结构化摘要模板

替换当前 `compact.rs` 中的 `COMPACT_PROMPT`，使用结构化模板：

```rust
const COMPACT_PROMPT: &str = r#"Summarize the conversation using this exact structure:

## Goal
用户的最终目标

## Progress
已完成的工作（包含具体文件路径、函数名）

## Key Decisions
做出的重要决策及原因

## Relevant Files
涉及的关键文件列表

## Current State
当前进展到哪一步

## Next Steps
接下来需要做什么

Requirements:
- Be specific: include file paths, function names, variable names
- Keep concise, roughly 20-30% of original length
- Focus on semantic meaning, not tool call details
"#;
```

### 3.2 工具调用对完整性修复

在 `compact.rs` 中新增 `sanitize_tool_pairs()` 函数，在压缩后调用：

- 遍历压缩后的消息列表
- 收集所有 `ToolUse` 的 id 和所有 `ToolResult` 的 `tool_use_id`
- 移除没有对应 `ToolUse` 的孤儿 `ToolResult`
- 为没有 `ToolResult` 的 `ToolUse` 添加占位符：`"[result removed during compaction]"`

### 3.3 头尾保护

改进 `compact_messages()` 的分割策略：

- **头部保护**：前 2 条消息（首轮对话）不参与压缩
- **尾部保护**：从末尾向前累积，保留最近 ~30% 上下文窗口的消息
- **中间部分**：送入 LLM 做结构化摘要
- 当前的"保留最近 40%"改为按 token 预算计算

### 3.4 代码改动

- `aemeath-core/src/compact.rs`：替换 `COMPACT_PROMPT`；新增 `sanitize_tool_pairs()`；改进 `compact_messages()` 分割逻辑

---

## 模块 4：渐进式 Skills 加载优化

### 4.1 条件过滤

Skill frontmatter 新增可选字段：

```yaml
---
name: deploy
description: 部署到生产环境
requires_tools: ["Bash"]
fallback_for: ["docker-deploy"]
---
```

- `requires_tools`：当所需工具不在注册表中时，隐藏该 skill
- `fallback_for`：当指定的主 skill 可用时，隐藏 fallback skill

### 4.2 进程内缓存

- 缓存已解析的 skill 列表（HashMap）
- 记录各文件的修改时间戳
- 再次调用 `load_all_skills()` 时，仅在文件变更时重新解析

### 4.3 代码改动

- `aemeath-core/src/skill.rs`：`Skill` 结构体新增 `requires_tools`、`fallback_for` 字段；`load_all_skills()` 增加过滤逻辑和缓存

---

## 模块 5：上下文安全扫描

### 5.1 威胁检测

在加载 CLAUDE.md 和外部 guidance 文件时，正则扫描已知 prompt injection 模式：

```rust
const THREAT_PATTERNS: &[(&str, &str)] = &[
    (r"ignore\s+(previous|all|above|prior)\s+instructions", "prompt_injection"),
    (r"do\s+not\s+tell\s+the\s+user", "deception"),
    (r"you\s+are\s+now\s+(?:a|an|DAN)", "jailbreak"),
    (r"system:\s*", "role_hijack"),
];
```

### 5.2 不可见字符检测

扫描零宽字符（U+200B-U+200F）、方向控制符（U+202A-U+202E）等不可见 Unicode 字符。

### 5.3 行为

- 检测到威胁 → 在注入内容前添加警告：`⚠️ [security: possible prompt injection detected in {filename}]`
- **不阻断加载**（避免误报影响正常使用），但让模型和用户知晓
- 日志记录检测结果

### 5.4 代码改动

- 新建 `aemeath-core/src/security.rs`：`scan_content(filename: &str, content: &str) -> Vec<SecurityWarning>`
- `aemeath-cli/src/main.rs`：加载 CLAUDE.md 和 guidance 文件时调用扫描

---

## 实现依赖关系

```
模块 1（模型行为纠正）  ← 无依赖，优先实现
模块 2（工具引导）       ← 已合并到模块 1
模块 3（压缩优化）       ← 无依赖，独立实现
模块 4（Skills 优化）    ← 无依赖，独立实现
模块 5（安全扫描）       ← 无依赖，独立实现
```

建议实现顺序：1 → 3 → 5 → 4（按影响从大到小）

---

## 改动文件清单

| 文件 | 改动类型 | 涉及模块 |
|------|---------|---------|
| `aemeath-core/src/config.rs` | 修改 | 1 |
| `aemeath-core/src/compact.rs` | 修改 | 3 |
| `aemeath-core/src/skill.rs` | 修改 | 4 |
| `aemeath-core/src/security.rs` | 新建 | 5 |
| `aemeath-core/src/lib.rs` | 修改 | 5 |
| `aemeath-cli/src/main.rs` | 修改 | 1, 5 |
