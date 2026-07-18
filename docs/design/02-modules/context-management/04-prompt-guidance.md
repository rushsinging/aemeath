# Context Management · Prompt & Guidance

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Context Management 私有 Prompt capability——系统提示组装、Guidance 解析、Skill 物化 integration、安全扫描与 prompt cache 稳定性。Prompt 不独立成 BC，也不向 Runtime / TUI 发布第二个 OHS；实现差距见 [迁移治理](../../03-engineering/03-migration-governance.md)。

## 1. 定位

Prompt 组装是 ContextPort `build_window` 的内部步骤之一：

```
build_window
  ├─ L2-L4 compact 读模型（L1 已在 ToolResult 入链前完成）
  ├─ prompt 组装（PromptPipeline）  ← 本文
  ├─ memory 注入（MemoryPort）
  └─ Context-owned block 编排 → ContextWindow.system_blocks + messages
```

- **PromptPipeline 是 Context-private 具体 capability**，不为不稳定的组装细节抽出第二个 OHS
- **Guidance 文件 I/O 是真实的外部 seam**：由 Context-owned `GuidanceSourcePort` 隔离发现、canonical path、缓存与异步 I/O；PromptPipeline **NEVER** 直接读文件系统
- **Skill 从供应方接入**：PromptPipeline 消费 Skill-owned `SkillMaterializationPort`；TUI picker 仍经 AgentClient / Skill catalog，**NEVER** 直调 PromptPipeline
- **不独立成 BC**：Prompt 是 Context Management 的支撑子域

## 2. 私有管线与外部 seam

```rust
struct PromptPipeline {
    guidance: Arc<dyn GuidanceSourcePort>,
    skills: Arc<dyn SkillMaterializationPort>,
}

impl PromptPipeline {
    /// 组装系统提示（guidance + discipline + skills + roles + git context）。
    async fn build_system_prompt(
        &self,
        req: &PromptRequest,
    ) -> Result<SystemPromptParts, PromptBuildError>;
}

#[async_trait]
trait GuidanceSourcePort: Send + Sync {
    /// 返回已按「_default → 短前缀 → 长前缀 → _reasoning」排序的快照。
    async fn materialize_model(
        &self,
        query: ModelGuidanceQuery,
    ) -> Result<GuidanceSnapshot, GuidanceSourceError>;

    /// 返回已按「全局 → 项目由远到近」排序的快照。
    async fn materialize_user(
        &self,
        query: UserGuidanceQuery,
    ) -> Result<GuidanceSnapshot, GuidanceSourceError>;
}

struct PromptRequest {
    system_prompt: SystemPromptSpec, // RunSpec → ContextRequest → PromptRequest 原值
    model_id: String,               // 用于 guidance 前缀匹配
    project_root: PathBuf,           // 项目根路径，只用于 user_guidance 寻址
    git_context: GitContextSnapshot, // Project snapshot 经 Context ACL 转换后的纯值 DTO
    lang: Language,                 // 中/英文
    effective_reasoning: ReasoningLevel,// Provider resolver 在 build 前冻结的最终值
    current_date: CalendarDate,     // ContextRequest 提供的本轮稳定值
    agents_roles: HashMap<String, AgentRoleConfig>,
    config_snapshot: ConfigSnapshot,
}

struct SystemPromptParts {
    /// Prompt capability 贡献的低频变化块；尚未设置 provider cache marker。
    cacheable: Vec<SystemBlock>,
    /// Prompt capability 贡献的本轮变化块。
    uncached: Vec<SystemBlock>,
    fingerprint: PromptFingerprint,
}

struct GuidanceSnapshot {
    documents: Vec<GuidanceDocument>,
    revision: GuidanceRevision,
}

struct GuidanceDocument {
    source: GuidanceSourceId,       // 稳定、可展示；不向领域层泄漏 fs handle
    canonical_id: CanonicalSourceId,// 用于去重
    content: String,
}
```

`ContextPort::build_window` **MUST** 等待 base system prompt、model guidance、user guidance 与 Skill 物化完成后才能产生窗口；任一供应失败都返回 typed error，**NEVER** 静默使用部分 prompt。文件 adapter **MAY** 按 mtime 缓存，但对 PromptPipeline 只暴露内容与单调 `revision`；这使同一管线无需同步 / 异步两套实现。Memory、active summary 与 Task reminder 不是 Prompt 素材：它们由 `build_window` 在 PromptPipeline 返回后依次取得并编排，**NEVER** 伪装成 `PromptRequest` 字段。

## 3. 系统提示组装管线

### 3.1 组装顺序

分 cacheable / uncached 的依据是**内容是否变化**，不是预先按类型分组。PromptPipeline 将低频变化的 user guidance / skills 等返回为 cacheable parts，将每轮可能变化的 current date / git context 返回为 uncached parts；Context Window assembler 再把 Memory / summary / Task projection 放到正确分段。唯一最终顺序如下：

```
┌─ cacheable_prefix ──────────────────────────────┐
│  1. system_prompt（RunSpec 的基础 prompt）        │
│  2. execution_discipline（编译时常量）            │
│  3. model_guidance（_default → 前缀 → reasoning）│
│  4. skills（supplier PromptFragment PL）          │
│  5. agent_roles                                  │
│  6. user_guidance（source revision 快照，见 §8）  │
│  7. memory_context（Context assembler 注入）     │
│  8. active_summary（Context assembler 注入）     │
└─────────────────────────────────────────────────┘
  ↓ cache_control 断点
┌─ uncached_suffix ───────────────────────────────┐
│  9. current_date（每轮变）                       │
│ 10. git_context（每轮可能变）                    │
│ 11. task_reminder（ContextRequest 纯值投影）      │
└─────────────────────────────────────────────────┘
```

### 3.2 cacheable 判定

每个组成部分独立判断是否变化：

| 组成部分 | 变化频率 | 变化检测方式 | cacheable |
|---|---|---|---|
| system_prompt | Run 内不变 | SystemPromptSpec fingerprint | 是 |
| execution_discipline | 不变（编译时常量） | — | 是 |
| model_guidance | 低（模型切换时变） | model_id 比对 | 是 |
| _default.md / _reasoning.md | 低（用户编辑文件时变） | guidance source revision | 是 |
| skills | 低（增删 skill 时变） | supplier materialization revision | 是 |
| agent_roles | 低（配置变更时变） | config snapshot 比对 | 是 |
| user_guidance | 低（用户编辑文件时变） | guidance source revision | 是 |
| memory_context | 中（reflection 写入时变） | Context assembler 的 entry fingerprint | 是（非 PromptRequest） |
| active_summary | 低（compact 时才变） | Context assembler 的 summary hash | 是（非 PromptRequest） |
| current_date | 每轮变 | — | 否 |
| git_context | 每轮可能变 | — | 否 |
| task_reminder | 每轮可能变 | ContextRequest value fingerprint | 否（非 PromptRequest） |

> **关键**：user_guidance、skills、memory 不是“动态所以不 cache”，而是“变化频率低，不变时 cache，变化时 miss 重算”。Memory / summary 的最终分段由 Context assembler 完成，不因此扩张 PromptRequest。

### 3.3 cache_control 断点

PromptPipeline **NEVER** 自行设置最终 cache breakpoint，因为 Memory / active summary 仍需插入 cacheable 段。Context Window assembler 按下列协议完成：

```rust
let parts = prompt.build_system_prompt(&prompt_request).await?;
let memory = render_memory(memory_port.retrieve_for_inject(&memory_query)?);
let summary = active_summary(&compaction_projection);

let mut blocks = parts.cacheable;
blocks.extend(memory);
blocks.extend(summary);
mark_cache_breakpoint(&mut blocks);       // 唯一 breakpoint
blocks.extend(parts.uncached);            // current_date / git_context
blocks.extend(render_task_reminder(&context_request.task_reminder));
```

- Provider ACL 把该逻辑 breakpoint 映射为 Anthropic messages API 的 `cache_control`
- cacheable_prefix 内容不变时命中缓存——即使 uncached_suffix 变化
- cacheable_prefix 中某部分变化时（如用户编辑了 AGENTS.md），整个 prefix cache miss，重算一次后下一轮恢复命中

## 4. Guidance 解析

### 4.1 文件结构

```
~/.agents/guidance/
├── _default.md          # 所有模型通用，总是加载
├── _reasoning.md        # reasoning_level != Off 时附加
└── {prefix}.md          # 按 model id 前缀匹配，所有匹配的都追加
```

> **`effective_reasoning`** 是本 invocation 已绑定的最终值：Runtime 先读取 `ReasoningPort.current_requested_level()`，再调用 Provider-owned resolver 按 model capability clamp，并在 `build_window` 前将 resolver 返回值同时冻结到 Context 与 Invocation。Prompt capability **NEVER** 读取或修改 Provider client 的“当前状态”；它只据 `effective_reasoning != Off` 决定是否加载 `_reasoning.md`。guidance 行为约束与 Provider adapter 的 wire 字段（`reasoning_effort` / `thinking.budget_tokens`）正交，但两者读取同一个 effective value。

### 4.2 组合加载策略

Guidance 采用**组合加载**（与 user_guidance 同策略），不是 fallback：

1. **`_default.md` 总是加载**——所有模型通用的系统 guidance
2. **所有前缀匹配的 `{prefix}.md` 都追加**——从最通用（最短前缀）到最具体（最长前缀），逐层叠加
3. **`_reasoning.md` 在 `effective_reasoning != Off` 时追加**

```rust
let model_guidance = self.guidance
    .materialize_model(ModelGuidanceQuery {
        model_id: req.model_id.clone(),
        reasoning_level: req.effective_reasoning,
        lang: req.lang,
    })
    .await?;
```

`GuidanceSourcePort` 的契约保证返回顺序、必选文件缺失语义与 stable source id；具体 file adapter 才负责 `read_dir`、前缀匹配、lang 分段解析、mtime 缓存与异步文件读取。PromptPipeline 只验证快照、扫描内容并渲染，**NEVER** 重复 adapter 的发现算法。

### 4.3 组装格式——带路径信息

每段 guidance 拼接时带来源路径，方便用户定位和调试：

```rust
fn render_guidance(documents: &[GuidanceDocument]) -> String {
    documents.iter()
        .map(|doc| {
            format!(
                "<guidance source=\"{}\">\n{}\n</guidance>",
                doc.source, doc.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

**组装示例**（model_id = `Zhipu/glm-5.2`，reasoning_level = High）：

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

- file adapter 按 request 的 `lang` 选择段落
- 无 lang 标记时全文加载
- lang 分段解析是 `GuidanceSourcePort` file adapter 的责任；该 adapter **MUST** 只有 async 物化路径，**NEVER** 维护行为可能漂移的同步同名实现

### 4.5 config-map 补充

当 `~/.agents/guidance/{prefix}.md` 文件不存在但 config 中有 `guidance_map` 条目时，从 config 补充（也是组合，不是 fallback）：

```rust
fn find_matching_config_guidance(model_id: &str, config: &ConfigSnapshot) -> Vec<(String, String)> {
    config.guidance_entries().iter()
        .filter(|(k, _)| model_id.starts_with(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
```

config-map 中的 guidance 条目同样按前缀长度升序追加到文件 guidance 之后。

## 5. Skill 物化

### 5.1 直接消费 Skill-owned PromptFragment PL

Prompt capability **NEVER** 定义第二份 `PromptFragment` / `SkillSummary`。唯一字段契约由 [Tool & Skill 领域模型](../tools/01-domain-model.md) 发布：`stable_key / content / source / cache_hint`。

### 5.2 加载管线

```
Skill-owned sources
  └─ SkillMaterializationPort::materialize_available(...).await
       └─ SkillMaterializationSnapshot { fragments, revision }
            └─ PromptPipeline: scan → dedup → budget → render
```

- Skill BC 独占 source discovery、builtin / file 合并、解析、能力校验与 I/O 缓存；Context **NEVER** 接收 Skill path 或自行打开 `SKILL.md`
- `revision` 只在物化结果变化时前进，PromptPipeline 以它作为 cacheable prefix 指纹的一部分
- PromptPipeline 按 `stable_key` 稳定去重（保留 supplier 顺序中的首个），用 `source` 记录扫描诊断，以 `content` 作为唯一注入正文；`cache_hint` 只是 supplier hint，最终 cache 分段仍由 Context 决定
- PromptPipeline 仍独占注入顺序、token budget、去重与安全扫描策略

### 5.3 渲染

```rust
fn render_skills(fragments: &[tools::PromptFragment], lang: Language) -> String {
    let header = skills_header(lang);  // "# 可用技能\n" / "# Available Skills\n"
    let body = dedup_by_stable_key(fragments).iter()
        .map(|fragment| fragment.content.as_str())
        .join("\n");
    format!("{}\n{}\n", header, body)
}
```

### 5.4 SKILL.md 安全扫描

所有 materialized skill content **MUST** 在进入 prompt candidate 前经过 `scan_content()`；发现 prompt injection 模式时记录 warning 与来源路径。扫描结果遵循 §7 的本地信任策略，不在 Prompt capability 复制另一套判定。

## 6. Git Context 注入

### 6.1 数据所有权

Prompt capability **NEVER** spawn git 或读取 process cwd。`PromptRequest` 只接收 Context Management 经 Project-owned `WorkspaceRead` 获取并 ACL 成 Context DTO 的 `GitContextSnapshot`；v0.1.0 至少包含 branch / workspace root / path base / worktree kind。若未来需要 status / log / diff，**MUST** 先由 Project 发布目的明确的窄能力，Context 再映射，**NEVER** 在 Prompt 内恢复散点 git 命令。

### 6.2 注入位置

git context 归入 `uncached_suffix`，因为 workspace 状态每轮可能变化。单次 `build_window` 只读取 request snapshot 一次；同一构建过程 **NEVER** 重探测。

## 7. 安全扫描

### 7.1 覆盖范围

| 文件类型 | Target 扫描要求 |
|---|---|
| user_guidance（AGENTS.md / CLAUDE.md） | MUST |
| config-map guidance | MUST |
| `_default.md` | MUST |
| `_reasoning.md` | MUST |
| `{prefix}.md` | MUST |
| SKILL.md | MUST |

### 7.2 策略

```rust
fn scan_guidance_document(source: &GuidanceSourceId, content: &str) -> ScanResult {
    let result = security::scan_content(content);
    if result.has_warnings() {
        log::warn!(
            target: "aemeath:agent:prompt",
            "Security scan warnings in guidance source {:?}: {:?}",
            source, result.warnings
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

> **命名统一**：`CLAUDE.md` 是 `AGENTS.md` 的兼容别名，设计文档统称 **user guidance**。每个目录优先 `AGENTS.md`；只有它不存在时才读取 `CLAUDE.md`。两个路径解析到同一 canonical file 时只加载一次。

User guidance 是用户编写的项目/全局指令文件，**不固化在 `PromptRequest` 中**。`PromptRequest` 只携带 `project_root`；每次 `build_window` 都经 `GuidanceSourcePort::materialize_user` 获取当前快照，adapter 可以用 mtime 避免重复 I/O。

#### 8.2.1 寻址规则

文件搜索分两层，**跨层加载多文件、同一目录只选一个兼容名称**：

```
全局层（home）：
  1. ~/.agents/AGENTS.md        ← 首选
  2. ~/.claude/CLAUDE.md         ← 仅当全局 AGENTS.md 不存在时 fallback

项目层（从 project_root 向上 N 级，含 project_root）：
 每层优先 AGENTS.md，fallback CLAUDE.md
 例如 project_root = /home/user/project/src：
    /home/user/project/src/AGENTS.md   或 CLAUDE.md
    /home/user/project/AGENTS.md       或 CLAUDE.md
    /home/user/AGENTS.md               或 CLAUDE.md
    ...（向上 N 级，默认 N=5）
```

#### 8.2.2 加载策略

跨目录命中的多文件同时加载，每文件独立快照；顺序固定为全局 → 项目祖先由远到近：

```rust
let user_guidance = self.guidance
    .materialize_user(UserGuidanceQuery {
        project_root: req.project_root.clone(),
        search_depth: req.config_snapshot.instruction_search_depth(),
        lang: req.lang,
    })
    .await?;
```

file adapter **MUST** 在物化过程中完成每目录首选 / fallback、canonical-path 去重与远到近排序，并将结果映射为通用 `GuidanceSnapshot`。

#### 8.2.3 缓存与 revision

具体 adapter **MAY** 以 canonical path + mtime 对每个文件独立缓存，但 mtime 是 adapter-private 细节。只有有序文档集合或内容变化时，`GuidanceSnapshot.revision` 才前进；PromptPipeline 以 revision 和内容指纹检测 cacheable prefix 变化。

#### 8.2.4 组装顺序与格式

与 model guidance（§4.3）同策略——每段带路径信息，从通用到具体逐层叠加。

```rust
fn render_user_guidance(snapshot: &GuidanceSnapshot) -> String {
    render_guidance(&snapshot.documents)
}
```

组装顺序为**全局 → 项目由远到近**（从最外层到最内层），使内层（更具体的）指令在文本末尾，LLM 更倾向于遵循。

**组装示例**（project_root = `/home/user/project/src`）：

```
<guidance source="~/.agents/AGENTS.md">
[全局用户指令：编码规范、语言偏好...]
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

若 `~/.agents/AGENTS.md` 不存在，则示例中的第一段改为 `~/.claude/CLAUDE.md`；两者 **NEVER** 在同一全局层同时加载。同理，每个项目目录至多贡献一份文件。canonical-path 去重还会消除软链或别名指向同一文件造成的重复注入。

#### 8.2.5 安全扫描

`combined` 组装后经过 `scan_content()` 检查，warnings 注入到日志（不阻止加载）。

#### 8.2.6 属于 cacheable_prefix

User guidance 归入 `cacheable_prefix`——变化频率低（用户偶尔编辑文件），大部分轮次 source revision 不变 → 命中 cache。用户编辑文件后下一轮 cache miss 重算一次，之后恢复命中。

## 9. Prompt Cache 稳定性

### 9.1 影响缓存命中的因素

| 因素 | 变化频率 | 影响层 | 变化检测 |
|---|---|---|---|
| system_prompt 变化 | Run 边界 | cacheable_prefix | SystemPromptSpec fingerprint |
| model_id 变化 | 低（用户切模型） | cacheable_prefix（guidance 重 resolve） | model_id 比对 |
| skill 增删 | 低 | cacheable_prefix（skill 列表重渲染） | supplier materialization revision |
| agent_roles 变化 | 低 | cacheable_prefix | config snapshot 比对 |
| user_guidance 变化 | 低（用户编辑文件） | cacheable_prefix | guidance source revision |
| memory 变化 | 中（reflection 写入时变） | Context-owned cacheable extension | entry fingerprint |
| active_summary 变化 | 低（compact 时才变） | Context-owned cacheable extension | summary hash |
| current_date | 每轮变 | uncached_suffix | — |
| git_context | 每轮可能变 | uncached_suffix | — |
| task_reminder | 每轮可能变 | Context-owned uncached extension | request value fingerprint |

### 9.2 缓存策略

- **Prompt parts**：PromptPipeline 只负责 RunSpec system prompt / execution discipline / guidance / skills / roles / date / git 的物化与 fingerprint，**NEVER** 缓存完整 Context Window。
- **Context-owned cacheable extension**：Memory 与 active summary 由 Context assembler 插在 Prompt cacheable parts 之后，并把 entry fingerprint / summary hash 纳入最终 window fingerprint；变化时 miss 一次，之后恢复命中。
- **Context-owned uncached extension**：Task reminder 位于 breakpoint 之后；它与 current date / git context 的变化不影响 prefix 命中。

### 9.3 模型切换时的缓存失效

模型切换 → `model_id` 变化 → guidance 重 resolve → cacheable_prefix 变化 → 缓存自动失效。这是正确行为，无需额外处理。

## 10. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图（Prompt capability = Context Management 私有支撑能力）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Current → Target 差距与退役责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：prompt 组装契约、guidance 解析、skill 物化、安全扫描、cache 稳定性 | #786 |
| 2026-07-14 | 收敛为 Context-private async PromptPipeline；只为 Guidance I/O 保留 Context-owned seam，Skill 经 supplier OHS 物化；Git 数据由 Project snapshot 经 ACL 注入；统一 user guidance 首选 / fallback、顺序与 canonical 去重 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | RunSpec system prompt 纳入唯一管线；直接消费 Skill-owned PromptFragment PL；冻结 Prompt→Memory 物化与最终 block 顺序 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
