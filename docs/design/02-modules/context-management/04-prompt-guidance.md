# Context Management · Prompt & Guidance

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 PromptPort OHS——系统提示组装的单一入口、Guidance 解析、Skill 物化、安全扫描覆盖与 prompt cache 稳定性。Prompt 是 Context Management BC 的支撑能力，不独立成 BC。

## 1. 定位

Prompt 组装是 ContextPort `build_window` 的内部步骤之一：

```
build_window
  ├─ L1-L4 compact 管线
  ├─ memory 注入（MemoryPort）
  ├─ prompt 组装（PromptPort）  ← 本文
  └─ → ContextWindow.system_blocks + messages
```

- **PromptPort 是独立端口**，由 ContextPort 内部组合调用
- **也被 Tool BC 间接使用**：skill 物化产出 PromptFragment，skill 列表供 TUI picker 消费
- **不独立成 BC**：Prompt 是 Context Management 的支撑子域

## 2. PromptPort trait

```rust
trait PromptPort: Send + Sync {
    /// 组装系统提示（guidance + discipline + skills + roles + git context）
    fn build_system_prompt(&self, req: &PromptRequest) -> SystemPrompt;

    /// 加载 skill 列表（供 TUI picker 和 prompt 注入）
    fn list_skills(&self) -> Vec<SkillSummary>;
}

struct PromptRequest {
    model_id: String,               // 用于 guidance 前缀匹配
    project_root: PathBuf,           // 项目根路径，用于 git context 和 user_guidance 寻址
    lang: Language,                 // 中/英文
    is_git_repo: bool,
    is_reasoning: bool,             // 是否加载 _reasoning.md guidance
    agents_roles: HashMap<String, AgentRoleConfig>,
    config_snapshot: ConfigSnapshot,
}

struct SystemPrompt {
    /// cacheable 前缀：内容不变时命中 Anthropic prompt cache
    cacheable_prefix: String,
    /// non-cacheable 后缀：每轮可能变化，放在 cache 断点之后
    uncached_suffix: String,
    /// cache_control 断点位置
    cache_breakpoint: Option<usize>,
}
```

### 2.1 替代关系

| 现状 | 目标 |
|---|---|
| `prompt::contract::PromptApiMarker`（空壳 ZST） | `PromptPort` trait |
| runtime 直接调 `prompt::api::*` 自由函数 | runtime 调 `PromptPort` trait 方法 |
| `build_static_prompt()` + `build_system_prompt_parts()` 双函数 | `PromptPort.build_system_prompt()` 单方法 |

## 3. 系统提示组装管线

### 3.1 组装顺序

分 cacheable / uncached 的依据是**内容是否变化**，不是预先按类型分组。低频变化的内容（user_guidance、skills、memory、active_summary）放在 cacheable 前缀中——内容不变时命中 cache，变化时 cache miss 重算一次。每轮可能变化的内容（current_date、git_context、task_reminder）放在 uncached 后缀。

```
┌─ cacheable_prefix ──────────────────────────────┐
│  1. execution_discipline（编译时常量）            │
│  2. model_guidance（前缀匹配 resolved）           │
│  3. _default.md                                  │
│  4. _reasoning.md（is_reasoning=true 时）         │
│  5. skills 列表（mtime 检测变化）                 │
│  6. agent_roles                                  │
│  7. user_guidance（多文件 mtime 快照，见 §8）     │
│  8. memory_context（fingerprint 检测变化）       │
│  9. active_summary（compact 时才变）             │
└─────────────────────────────────────────────────┘
  ↓ cache_control 断点
┌─ uncached_suffix ───────────────────────────────┐
│ 10. current_date（每轮变）                       │
│ 11. git_context（每轮可能变）                    │
│ 12. task_reminder（每轮可能变）                  │
└─────────────────────────────────────────────────┘
```

### 3.2 cacheable 判定

每个组成部分独立判断是否变化：

| 组成部分 | 变化频率 | 变化检测方式 | cacheable |
|---|---|---|---|
| execution_discipline | 不变（编译时常量） | — | ✅ |
| model_guidance | 低（模型切换时变） | model_id 比对 | ✅ |
| _default.md / _reasoning.md | 低（用户编辑文件时变） | mtime 检查 | ✅ |
| skills | 低（增删 skill 时变） | 目录 mtime 检查 | ✅ |
| agent_roles | 低（配置变更时变） | config snapshot 比对 | ✅ |
| user_guidance | 低（用户编辑文件时变） | 逐文件 mtime 检查 | ✅ |
| memory_context | 中（reflection 写入时变） | entry fingerprint 比对 | ✅ |
| active_summary | 低（compact 时才变） | summary hash 比对 | ✅ |
| current_date | 每轮变 | — | ❌ |
| git_context | 每轮可能变 | — | ❌ |
| task_reminder | 每轮可能变 | — | ❌ |

> **关键**：user_guidance、skills、memory 不是"动态所以不 cache"，而是"变化频率低，不变时 cache，变化时 miss 重算"。把它们放在 cacheable 前缀中，大部分轮次命中 cache。

### 3.3 cache_control 断点

在 `cacheable_prefix` 末尾设置 `cache_control: { type: "ephemeral" }`：

```rust
SystemPrompt {
    cacheable_prefix: "...",       // 低频变化，不变时命中 cache
    cache_breakpoint: Some(cacheable_prefix.len()),
    uncached_suffix: "...",        // 每轮可能变
}
```

- Anthropic messages API 的 `cache_control` 标记 cacheable 前缀
- cacheable_prefix 内容不变时命中缓存——即使 uncached_suffix 变化
- cacheable_prefix 中某部分变化时（如用户编辑了 AGENTS.md），整个 prefix cache miss，重算一次后下一轮恢复命中

## 4. Guidance 解析

### 4.1 文件结构

```
~/.agents/guidance/
├── _default.md          # 所有模型通用，总是加载
├── _reasoning.md        # is_reasoning=true 时附加
└── {prefix}.md          # 按 model id 前缀匹配，所有匹配的都追加
```

> **`is_reasoning`** 由 Runtime 从 ProviderPort 的 `current_reasoning_level()` 获取（模型配置决定），传入 `PromptRequest`。PromptPort 据此决定是否加载 `_reasoning.md`——该文件包含 reasoning 模式下的行为指导（如推理简洁性、语言选择等），是 guidance 层面的关注点，与 provider 侧的 API 参数（`reasoning_effort` / `thinking.budget_tokens`）正交。

### 4.2 组合加载策略

Guidance 采用**组合加载**（与 user_guidance 同策略），不是 fallback：

1. **`_default.md` 总是加载**——所有模型通用的系统 guidance
2. **所有前缀匹配的 `{prefix}.md` 都追加**——从最通用（最短前缀）到最具体（最长前缀），逐层叠加
3. **`_reasoning.md` 在 `is_reasoning=true` 时追加**

```rust
struct GuidanceFile {
    path: PathBuf,
    content: String,
    mtime: SystemTime,
}

struct GuidanceSnapshot {
    files: Vec<GuidanceFile>,    // 按「_default → 短前缀 → 长前缀 → _reasoning」顺序
    combined: String,            // 组装结果，每段带路径信息
}

fn resolve_guidance(model_id: &str, guidance_dir: &Path, is_reasoning: bool) -> GuidanceSnapshot {
    let mut files: Vec<GuidanceFile> = Vec::new();

    // 1. _default.md 总是加载
    let default_path = guidance_dir.join("_default.md");
    if let Some(f) = read_with_mtime(&default_path) {
        files.push(f);
    }

    // 2. 所有前缀匹配的 {prefix}.md，按前缀长度升序（通用 → 具体）
    let mut matches: Vec<(usize, PathBuf)> = fs::read_dir(guidance_dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.trim_end_matches(".md");
            if stem.starts_with('_') { return None; }  // 跳过 _default/_reasoning
            if model_id.starts_with(stem) {
                Some((stem.len(), e.path()))
            } else {
                None
            }
        })
        .collect();
    matches.sort_by(|a, b| a.0.cmp(&b.0));  // 升序：短前缀（通用）在前
    for (_, path) in matches {
        if let Some(f) = read_with_mtime(&path) {
            files.push(f);
        }
    }

    // 3. _reasoning.md（is_reasoning=true 时）
    if is_reasoning {
        let reasoning_path = guidance_dir.join("_reasoning.md");
        if let Some(f) = read_with_mtime(&reasoning_path) {
            files.push(f);
        }
    }

    let combined = render_guidance_with_paths(&files);
    GuidanceSnapshot { files, combined }
}
```

### 4.3 组装格式——带路径信息

每段 guidance 拼接时带来源路径，方便用户定位和调试：

```rust
fn render_guidance_with_paths(files: &[GuidanceFile]) -> String {
    files.iter()
        .map(|f| {
            let path_display = f.path.strip_prefix(home_dir())
                .unwrap_or(&f.path)
                .display();
            format!(
                "<guidance source=\"~{}\">\n{}\n</guidance>",
                path_display, f.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

**组装示例**（model_id = `Zhipu/glm-5.2`，is_reasoning = true）：

```
<guidance source="~/.agents/guidance/_default.md">
[通用执行规则、输出格式约束...]
</guidance>

<guidance source="~/.agents/guidance/Zhipu.md">
[GLM 系列通用 guidance：中文偏好、工具调用习惯...]
</guidance>

<guidance source="~/.agents/guidance/Zhipu/glm.md">
[GLM 模型特定 guidance：上下文窗口策略、角色定位...]
</guidance>

<guidance source="~/.agents/guidance/Zhipu/glm-5.md">
[GLM-5 版本特定 guidance：新能力提示...]
</guidance>

<guidance source="~/.agents/guidance/_reasoning.md">
[推理模式 guidance：推理简洁性、中文推理...]
</guidance>
```

> 组装顺序从通用到具体——后加载的更具体 guidance 可以覆盖或补充前面的。用户在具体 guidance 中写的内容优先级更高（因为出现在 prompt 末尾，LLM 更倾向遵循）。

### 4.4 Lang-aware 加载

每个 guidance 文件可包含 lang 标记：

```
# Zhipu.md
[zh]
你是 GLM 模型...
[en]
You are GLM model...
```

- `load_named_file_async_with_lang(path, lang)` 按 lang 选择段落
- 无 lang 标记时全文加载
- **sync `resolve_guidance` 退役**——当前 sync 版本是 unmaintained drift bait，async 版本是唯一路径

### 4.5 config-map 补充

当 `~/.agents/guidance/{prefix}.md` 文件不存在但 config 中有 `guidance_map` 条目时，从 config 补充（也是组合，不是 fallback）：

```rust
fn find_matching_config_guidance(model_id: &str, config: &ConfigSnapshot) -> Vec<(String, String)> {
    config.guidance_map.iter()
        .filter(|(k, _)| model_id.starts_with(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
```

config-map 中的 guidance 条目同样按前缀长度升序追加到文件 guidance 之后。

## 5. Skill 物化

### 5.1 Skill → PromptFragment

```rust
struct SkillSummary {
    name: String,
    aliases: Vec<String>,
    description: String,
    source: SkillSource,            // File | Builtin
}

struct PromptFragment {
    name: String,
    content: String,                // 渲染为 "- `name` (aliases: /a, /b): description"
}
```

### 5.2 加载管线

```
~/.agents/skills/*.md     ─┐
.agents/skills/*.md        ├─→ load_all_skills() → Vec<Skill> → PromptFragment
builtin skills (commit)   ─┘
```

- `load_all_skills_cached()` — 带缓存的全量加载
- 缓存失效：文件 mtime 变化时重载
- builtin skill（commit）：`content` 通过 `read_skill_content("aemeath-builtin://commit")` 获取

### 5.3 渲染

```rust
fn render_skills(skills: &[Skill], lang: Language) -> String {
    let header = skills_header(lang);  // "# 可用技能\n" / "# Available Skills\n"
    let body = skills.iter()
        .filter(|s| !s.requires_tools.is_empty() == false)  // 简化：全部展示
        .map(|s| format!("- `{}` (aliases: {}): {}",
            s.name,
            s.aliases.iter().map(|a| format!("/{}", a)).join(", "),
            s.description))
        .join("\n");
    format!("{}\n{}\n", header, body)
}
```

### 5.4 SKILL.md 安全扫描

**当前 gap**：安全扫描 `scan_content()` 未覆盖 SKILL.md 文件。

**目标**：所有 skill content 在加载时经过 `scan_content()` 检查，发现 prompt injection 模式时发出 warning 日志（不阻止加载，但记录）。

## 6. Git Context 注入

### 6.1 当前实现

`collect_git_context(project_root)` 串行执行 5 个 git 子命令：

| 命令 | 产出 |
|---|---|
| `git rev-parse --abbrev-ref HEAD` | 当前分支名 |
| `git status --short` | 工作区状态 |
| `git log --oneline -5` | 最近 5 条 commit |
| `git diff --stat` | 未暂存变更统计 |
| `git diff --cached --stat` | 已暂存变更统计 |

### 6.2 目标优化

| 优化 | 优先级 | 说明 |
|---|---|---|
| 缓存 | P2 | 同一 project_root + 同一 git HEAD 时复用（git context 在单次 Run 内不变） |
| 并行 | P3 | 5 个子命令无依赖，可并行执行 |
| 精简 | P2 | `git diff --stat` 在大仓库可能很长，加 head limit |

### 6.3 注入位置

git context 归入 `uncached_suffix`，因为 git status 每轮可能变化。

## 7. 安全扫描

### 7.1 覆盖范围

| 文件类型 | 当前覆盖 | 目标 |
|---|---|---|
| user_guidance（AGENTS.md / CLAUDE.md） | ✅ | ✅ |
| config-map guidance | ✅ | ✅ |
| `_default.md` | ❌ | ✅ |
| `_reasoning.md` | ❌ | ✅ |
| `{prefix}.md` | ❌ | ✅ |
| SKILL.md | ❌ | ✅ |

### 7.2 策略

```rust
fn scan_guidance_file(path: &Path, content: &str) -> ScanResult {
    let result = security::scan_content(content);
    if result.has_warnings() {
        log::warn!(
            target: "aemeath:agent:prompt",
            "Security scan warnings in guidance file {:?}: {:?}",
            path, result.warnings
        );
    }
    // 不阻止加载——guidance 是用户信任的本地文件
    // 但 warning 进入日志供审计
    result
}
```

- **不阻止加载**：guidance 文件是用户本地信任文件，scan 只做审计记录
- **阻止加载**：仅对远程获取的 guidance（v0.1.0 无此场景）

## 8. Execution Discipline 与 User Guidance

### 8.1 Execution Discipline

编译时常量（`share::i18n::prompt::discipline`），所有模型共用，按 lang 选择中/英文版本。内容包括：

- 工具调用纪律（先 Read 再 Edit、不猜测文件内容等）
- 输出格式约束（act_dont_describe、不主动生成测试/文档等）
- 任务分类纪律（INTERRUPT > NEW REQUEST > CLARIFICATION > ASIDE）
- 安全规则（不执行恶意命令等）

**不按模型区分**——这是跨模型的通用纪律，不可被 guidance 覆盖。模型特定行为通过 `{prefix}.md` guidance 覆盖。

### 8.2 User Guidance（用户自定义指令）

> **命名统一**：`CLAUDE.md` 和 `AGENTS.md` 是同一概念的历史别名，设计文档中统称 **user guidance**。当前代码两者兼容加载（CLAUDE.md 优先），目标是逐步收敛到 `AGENTS.md` 单一命名。

User guidance 是用户编写的项目/全局指令文件，**不固化在 `PromptRequest` 中**——每次 `build_window` 时动态读取，各有各的 mtime 快照。

#### 8.2.1 寻址规则

文件搜索分两层，**同时加载多文件**（不是取第一个存在的就 break）：

```
全局层（home）：
  1. ~/.agents/AGENTS.md        ← 首选
  2. ~/.claude/CLAUDE.md         ← 兼容 fallback

项目层（从 project_root 向上 N 级，含 project_root）：
 每层优先 AGENTS.md，fallback CLAUDE.md
 例如 project_root = /home/user/project/src：
    /home/user/project/src/AGENTS.md   或 CLAUDE.md
    /home/user/project/AGENTS.md       或 CLAUDE.md
    /home/user/AGENTS.md               或 CLAUDE.md
    ...（向上 N 级，默认 N=5）
```

#### 8.2.2 加载策略

**当前问题**：全局和项目各只取第一个存在的文件（`break`），忽略同层可能的另一个文件，也忽略更上层目录的文件。

**目标**：多文件同时加载，每文件独立快照：

```rust
struct UserGuidanceFile {
    path: PathBuf,
    content: String,
    mtime: SystemTime,
}

struct UserGuidanceSnapshot {
    files: Vec<UserGuidanceFile>,     // 按「全局 → 项目由远到近」顺序
    combined: String,                 // join("\n\n") 后的组装结果
}

fn load_user_guidance(project_root: &Path) -> UserGuidanceSnapshot {
    let mut files: Vec<UserGuidanceFile> = Vec::new();

    // 全局层：两个文件都加载（如果都存在）
    for global_path in &[
        paths::global_agents_md_path(),
        paths::old_global_claude_md_path(),
    ] {
        if let Some(file) = read_with_mtime(global_path) {
            files.push(file);
        }
    }

    // 项目层：从 project_root 向上 N 级，每层 AGENTS.md + CLAUDE.md 都加载
    for dir in paths::project_instruction_dirs(project_root, INSTRUCTION_SEARCH_DEPTH) {
        for file_name in &[paths::AGENTS_MD, paths::CLAUDE_MD] {
            let path = dir.join(file_name);
            if let Some(file) = read_with_mtime(&path) {
                files.push(file);
            }
        }
    }

    let combined = render_user_guidance_with_paths(&files);

    UserGuidanceSnapshot { files, combined }
}
```

#### 8.2.3 mtime 缓存

每个文件独立缓存，避免每轮 IO：

```rust
fn read_with_mtime(path: &Path) -> Option<UserGuidanceFile> {
    let mtime = fs::metadata(path).ok()?.modified().ok()?;

    // 缓存命中：mtime 未变化时复用
    if let Some(cached) = CACHE.get(path) {
        if cached.mtime == mtime {
            return Some(cached.clone());
        }
    }

    // 缓存未命中：读取文件
    let content = fs::read_to_string(path).ok()?;
    let file = UserGuidanceFile {
        path: path.to_path_buf(),
        content,
        mtime,
    };
    CACHE.insert(path, file.clone());
    Some(file)
}
```

#### 8.2.4 组装顺序与格式

与 model guidance（§4.3）同策略——每段带路径信息，从通用到具体逐层叠加。

```rust
fn render_user_guidance_with_paths(files: &[UserGuidanceFile]) -> String {
    files.iter()
        .map(|f| {
            let path_display = f.path.strip_prefix(home_dir())
                .unwrap_or(&f.path)
                .display();
            format!(
                "<guidance source=\"~{}\">\n{}\n</guidance>",
                path_display, f.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

组装顺序为**全局 → 项目由远到近**（从最外层到最内层），使内层（更具体的）指令在文本末尾，LLM 更倾向于遵循。

**组装示例**（project_root = `/home/user/project/src`）：

```
<guidance source="~/.agents/AGENTS.md">
[全局用户指令：编码规范、语言偏好...]
</guidance>

<guidance source="~/.claude/CLAUDE.md">
[兼容的历史全局指令...]
</guidance>

<guidance source="/home/user/AGENTS.md">
[用户主目录级指令：通用项目约定...]
</guidance>

<guidance source="/home/user/project/AGENTS.md">
[项目级指令：架构规范、模块边界...]
</guidance>

<guidance source="/home/user/project/src/AGENTS.md">
[子目录级指令：当前模块特定规则...]
</guidance>
```

#### 8.2.5 安全扫描

`combined` 组装后经过 `scan_content()` 检查，warnings 注入到日志（不阻止加载）。

#### 8.2.6 属于 cacheable_prefix

User guidance 归入 `cacheable_prefix`——变化频率低（用户偶尔编辑文件），大部分轮次 mtime 不变 → 命中 cache。用户编辑文件后下一轮 cache miss 重算一次，之后恢复命中。

## 9. Prompt Cache 稳定性

### 9.1 影响缓存命中的因素

| 因素 | 变化频率 | 影响层 | 变化检测 |
|---|---|---|---|
| model_id 变化 | 低（用户切模型） | cacheable_prefix（guidance 重 resolve） | model_id 比对 |
| skill 增删 | 低 | cacheable_prefix（skill 列表重渲染） | 目录 mtime |
| agent_roles 变化 | 低 | cacheable_prefix | config snapshot 比对 |
| user_guidance 变化 | 低（用户编辑文件） | cacheable_prefix | 逐文件 mtime 检查 |
| memory 变化 | 中（reflection 写入时变） | cacheable_prefix | entry fingerprint 比对 |
| active_summary 变化 | 低（compact 时才变） | cacheable_prefix | summary hash 比对 |
| current_date | 每轮变 | uncached_suffix | — |
| git_context | 每轮可能变 | uncached_suffix | — |
| task_reminder | 每轮可能变 | uncached_suffix | — |

### 9.2 缓存策略

- **cacheable_prefix**：所有低频变化内容放在 cache 断点之前。各部分独立检测变化——内容不变时命中 cache，变化时 miss 重算一次后恢复命中
- **uncached_suffix**：current_date / git_context / task_reminder 每轮可能变，放在 cache 断点之后，不影响 prefix 缓存命中
- **memory 注入**：放在 cacheable_prefix 中，通过 fingerprint 检测变化——reflection 写入新 memory 时 fingerprint 变化 → cache miss 一次 → 下一轮恢复命中

### 9.3 模型切换时的缓存失效

模型切换 → `model_id` 变化 → guidance 重 resolve → cacheable_prefix 变化 → 缓存自动失效。这是正确行为，无需额外处理。

## 10. 现状端口缺口

| 目标 | 现状 | 迁移动作 |
|---|---|---|
| `PromptPort` trait | ❌ 无，runtime 直接调 `prompt::api::*` | 抽 trait，实现移到 adapter |
| `PromptPort.build_system_prompt()` | ⚠️ `build_static_prompt()` + `build_system_prompt_parts()` 双函数 | 合并为单方法 |
| 安全扫描全覆盖 | ⚠️ 部分覆盖 | 扩展到所有 guidance + SKILL.md |
| sync `resolve_guidance` 退役 | ⚠️ 存在但未用 | 删除 sync 版本 |
| `PromptApiMarker` 退役 | ⚠️ 空壳 | 被 `PromptPort` 替代后删除 |
| git context 缓存 | ❌ 无 | 加 per-Run 缓存 |
| `static_part` 重复计算 | ⚠️ `build_static_prompt` 丢弃 `SystemPromptParts.static_part` | 修复：不重复计算 |
| user_guidance 多文件加载 | ⚠️ 当前全局/项目各只取第一个存在文件 | 改为多文件同时加载 + 每文件独立 mtime 快照 |
| user_guidance 动态快照 | ⚠️ 当前作为字符串固化传入 PromptRequest | 改为 build_window 时动态读取，不固化在 PromptRequest |

## 11. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图（PromptPort = Context Management 支撑）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：PromptPort trait、组装管线、guidance 解析、skill 物化、安全扫描、cache 稳定性 | #786 |
