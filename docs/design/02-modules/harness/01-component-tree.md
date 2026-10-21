# Harness · 组件树与注册表

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义 8 个可编辑组件的 aemeath 形态与 ComponentRegistry 统一契约。所有组件读写 MUST 经 registry，NEVER 散落直连文件。

## 1. 组件清单（aemeath 形态）

| # | 组件 | 承载物 | 上下文成本 | 可编辑面 |
|---|---|---|---|---|
| c1 | systemprompt overlay | 全局 `~/.agents/AGENTS.md` + 项目 AGENTS.md/CLAUDE.md 链（5 级祖先遍历） | ALWAYS ON | 仅 overlay；STATIC_SYSTEM_PROMPT 本体编译期固定，NEVER 开放 |
| c2 | guardrails | 全局 `~/.agents/guardrails.md` + 项目 `.agents/guardrails.md`（新增） | ALWAYS ON | 行为规则 prose；与 policy（能力门控）分层，NEVER 合并 |
| c3 | memory | `~/.agents/memory/` corpus | 注入时 | 由 memory reflect 治理；registry 提供只读视图，RSI NEVER 直接改写 |
| c4 | tool_guidance 覆盖层 | `~/.agents/tool_guidance.md`（新增） | ALWAYS ON | 仅使用指南 prose；工具 schema/描述本体编译期固定，NEVER 外置 |
| c5 | skills | 全局 `~/.agents/skills/` + 项目 `.agents/skills/`、`.claude/skills/` | 按需 | 单个 `SKILL.md` 整文件（frontmatter 校验对齐现有加载器） |
| c6 | sub_agents | `aemeath.json` 的 agents 定义（names + role 绑定） | 派发时 | config 热载生效（见 02 分片），RSI 编辑后置 |
| c7 | scripts | `~/.agents/scripts/` + 项目 `.agents/scripts/`（新增） | 按需 | 单脚本文件 + frontmatter；引用可见性规则见 05 分片 |
| c8 | plugin | 预留位（0.3.0） | loop 内 | seam + tool loop 中间件 + TUI 渲染，本里程碑不落地 |

## 2. ComponentRegistry 契约

```rust
enum ComponentKind { SystemPromptOverlay, Guardrails, MemoryView, ToolGuidance,
                     Skill, SubAgents, Script, Plugin /* 预留 */ }

struct ComponentId { kind: ComponentKind, name: ComponentName }  // name 用于 skills/scripts 等多实例组件

trait ComponentRegistry {
    fn list(&self) -> Vec<ComponentId>;
    fn read(&self, id: &ComponentId) -> Result<ComponentContent>;
    fn validate(&self, id: &ComponentId, content: &ComponentContent) -> Result<()>;
    fn write(&mut self, id: &ComponentId, content: &ComponentContent) -> Result<()>;  // 内建原子写
    fn diff(&self, old: &ComponentContent, new: &ComponentContent) -> ComponentDiff;
}
```

- `ComponentContent`：结构化正文（frontmatter 字段 + body），非裸字节。
- 每个组件一个 adapter（对齐 hexagonal：registry 是 application 层端口，文件系统是 adapter）。
- `validate` 规则按组件分注册（如 skill：frontmatter `name` 与目录一致、`description` 非空；script：语法预检）。

## 3. 编辑契约：omitted-is-inherited

```rust
struct ChangeSet { edits: Vec<ComponentEdit> }   // 提案只携带变更项
enum ComponentEdit { Upsert { id, content }, Delete { id } }
```

- **未列出的组件自动继承当前版本**——提案 NEVER 要求携带全量组件树。
- **校验失败整份 ChangeSet 作废**：任一 edit 的 validate 失败，全部 edit 拒绝（对齐 harness2「THREE RULES THAT DISCARD THE WHOLE PROPOSAL」），NEVER 部分采纳。
- 写入顺序：全部 edit 先 validate，再统一 write（见 02 分片热载生效）。

## 4. 原子写

- `write` 实现：临时文件 + rename（tmp+rename），NEVER 直接覆写在线文件，防热载读到半写状态。
- rename 前完成 validate；rename 后即对下一次 run 边界检测可见。

## 5. 不变量

1. STATIC_SYSTEM_PROMPT、工具 schema/描述本体 NEVER 出现在 registry 可写面。
2. registry 是组件读写的**唯一**通道；TUI 诊断视图与 RSI daemon 均经此（c3 memory 只读视图除外，其写路径归 memory 模块）。
3. `list()` 结果稳定排序（kind, name 升序），保证 diff 与快照可复现。
