# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## Constitution

1. 指令使用 **NEVER**、**MUST**、**SHOULD**、**MAY** 表示约束等级：
   - **NEVER**：绝对禁止，无例外。
   - **MUST**：强制要求。违反会导致构建失败、数据丢失或行为错误。
   - **SHOULD**：强烈推荐。跳过时必须有明确理由。
   - **MAY**：可选。有助于清晰度或便利性时使用。
2. 所有新增指令 **MUST** 遵循此约定。
3. 引入新规则或约定时，**MUST** 先征得用户同意再更新本文件或任一 `specs/` 分片。
4. **MUST** 遵循 DRY 原则。类型、辅助函数、常量、数据访问逻辑 **MUST** 定义一次、到处引用。**NEVER** 在多个文件重复相同逻辑。
5. **MUST NOT** 手动调整代码格式。格式化由 `cargo fmt` / `rustfmt` 处理，只关注逻辑变更。
6. **MUST** 遵循 TDD（测试先行）。新增或修改核心逻辑前，**MUST** 先新增或修改对应测试（feature 先写表达期望行为的测试、bug 先写复现问题的失败测试、重构先确认现有测试覆盖目标行为），再用 `cargo test` 验证修改结果；细节见 `specs/rust-coding.md`。
7. **MUST** 跨层链路改动 **MUST** 为**每一层**补单元测试或场景测试，**NEVER** 只测首尾。跨 share → runtime → sdk → tui 的数据流改动，若只测源头 emit 和最终渲染，中间任一层（SDK 转发、adapter 分发、model 写入）的覆写 / 绕过 / 字段丢失都无法被测试捕获。典型反面教材：PR #635 数据全链路正确但 TUI 不显示，根因是 `AgentDisplay::format_header_line_with_result` 覆写了 trait 默认方法绕过了消费逻辑——因为链路中每一层都没有测试覆盖。

## 渐进式披露（Progressive Disclosure）

根指令（本文件）覆盖全仓库，始终适用。`specs/` 下的分片承载特定工作的 detailed 规则。**开始工作前**先加载匹配的分片，再动手。

两种触发机制，按序应用：

1. **路径触发（主，机械式）**：当工作会读、改或运行某条路径前缀下的代码时，**MUST** 加载对应分片。路径重叠即触发，无需判断。
2. **场景触发（次，语义式）**：当工作触及某分片的范围但未碰它的路径——典型是别处在*消费*该能力（如改 provider 默认值会影响 `prompt` 的 guidance 选择）——也 **MUST** 加载。

多行命中时，加载**全部**匹配分片。分片之间若冲突，停下来问用户，**NEVER** 自行取舍。

### 架构地图与触发表

| 分片 | 角色 / 路径（主触发） | 同时加载（次触发） |
|---|---|---|
| `specs/rust-coding.md` | 横切，任意 `**/*.rs`、`**/Cargo.toml` —— 编码 / 测试 / 日志 / 验证门禁 / 错误处理 | 跑验证门禁、新增任何核心逻辑、调试构建或测试失败 |
| `specs/tui-cli.md` | `apps/cli/src/**` —— TUI（ratatui）、旧版 REPL（rustyline）、主题色板（Catppuccin Macchiato） | 改输入 / 渲染 / 快捷键 / 选区复制，或新增工具显示（`ToolDisplayEntry`），或改颜色 / 主题 |
| `specs/runtime.md` | `agent/features/runtime/**` —— Agent 循环、tool 执行编排、token budget、compact、成本、slash 命令 | 改暂停 / 恢复 / 重试（同步更新 `token_estimation`）、成本追踪（同步更新 `pricing.rs`）、新增 slash 命令 |
| `specs/tools.md` | `agent/features/tools/**` —— `Tool` trait、`ToolRegistry`、MCP 主体 | 新增内置 Tool，或改 MCP 工具加载 / 注册 |
| `specs/provider.md` | `agent/features/provider/**` —— provider 的 HTTP / stream 实现 | 新增 provider（同步加 model guidance 文件，并在 `config-compat` 补默认值） |
| `specs/prompt.md` | `agent/features/prompt/**` —— Guidance 系统、系统提示、上下文注入 | 改 provider 默认 model（影响 guidance 前缀匹配），或改系统提示注入 |
| `specs/project.md` | `agent/features/project/**` —— worktree 工作区上下文（`WorkspaceService` 单一可变状态源、COLA 分层、git 出站端口） | 改 worktree 进入 / 退出 / 持久化（同步 `storage.md` 会话落盘），或经 slash 命令操作 worktree（涉及 `runtime.md`） |
| `specs/config-compat.md` | `agent/shared/src/config/**` —— 配置分层、provider 默认值 / env / base_url、Claude Code 兼容、运行时路径 | 新增 `AEMEATH_*` 配置项，或改指令 / 配置 / skills / hooks 的读取优先级 |
| `specs/storage.md` | `agent/features/storage/**` —— memory、task、history、tool_result 持久化 | 改会话 / 记忆 / 任务 / 历史的落盘格式或路径 |
| `specs/policy-hook-audit.md` | `agent/features/{policy,hook,audit}/**` —— 权限评估、hook 执行、审计 | 改 hook 执行环境变量注入（`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`） |
| `specs/update.md` | `agent/features/update/**` —— 版本检查（GitHub API + semver + 缓存）、`aemeath update` 子命令 | 改版本检查逻辑 / 缓存策略 / 更新渠道配置 |
| `specs/bug-feature-tracking.md` | 无路径触发 | 任何 bug 修复或 feature 实现；操作 GitHub Issues（迁移自 `docs/bug/`、`docs/snapshot/`） |
| `specs/logging.md` | `packages/global/logging/**`、全仓库 `log::xxx!` 调用点 —— 日志 target 命名、14 字段 schema、event_type 枚举、级别策略 | 新增/修改 log 调用、改日志路由、改 schema 字段、新增日志文件 |
| `docs/design/02-architecture-guards.md` | `.agents/aemeath.json`、`.agents/hooks/**` —— 架构守卫注册表与 17 个 guard 脚本 | 新增 / 调整守卫、白名单、Hook 编排；Stop 时 `check-architecture-guards.sh` 失败需排查；改 `docs/design/02-architecture-guards.md` 本身 |

> `agent/shared/**`（除 `config/`）、`agent/composition/**`、`packages/**` 的改动按内容落到最相关分片；纯横切改动至少加载 `rust-coding.md`。

## 项目结构

```
aemeath/                    # workspace root
├── apps/
│   └── cli/                # CLI 二进制入口 + TUI (ratatui) + 旧版 REPL (rustyline)
├── agent/
│   ├── features/           # 业务 feature：runtime/tools/provider/prompt/project/storage/policy/hook/audit/update
│   ├── shared/             # 横切基础设施 + 外部 adapter + 最小共享内核
│   └── composition/        # 组合根：唯一生产装配入口
├── packages/
│   ├── sdk/                # AgentClient trait + 公共类型（CLI↔Runtime 通信契约）
│   └── global/logging/     # 日志 projection 适配
├── docs/                   # 设计真相与历史归档
│   ├── design/             #   设计真相源：outline / runtime / tui / server / file-split-plan / architecture-guards
│   ├── snapshot/           #   历史 spec 快照（已废弃、不参与运行时）
│   ├── superpowers/        #   brainstorm / superpowers 工作流产物
│   ├── mockups/            #   UI 草图
│   └── visual/             #   可视化资产
├── specs/                  # 渐进式披露分片：按需加载的 detailed 规则
├── TODO.md                 # 待办事项（通过 /todo 命令维护）
└── AGENTS.md               # 本文件（CLAUDE.md 软链指向它）
```

> bug / feature 追踪改在 GitHub Issues（仓库 `rushsinging/aemeath`），**NEVER** 再向 `docs/` 写入新的 bug / feature 条目。

## 运行时目录（`~/.agents`）

```
~/.agents/                   # 运行时根目录
├── aemeath.json             # 全局配置
├── AGENTS.md                # 全局指令
├── guidance/                # 模型 Guidance 文件
│   ├── _default.md          #   所有模型通用
│   ├── _reasoning.md        #   reasoning 开启时附加
│   └── {prefix}.md          #   按 model id 前缀匹配（最长优先）
├── hooks/                   # 全局 Hook 脚本
├── logs/                    # 日志文件（12 个 target 路由文件 + 兜底 + panic）
│   ├── aemeath.log          #   兜底日志（不匹配任何合法 target 时路由到此）
│   ├── panic.log            #   Panic 日志（panic_hook.rs 直写，不纳入 UnifiedLogger）
│   ├── tui.log              #   aemeath:tui — TUI / CLI 入口
│   ├── shared.log           #   aemeath:shared — shared 层
│   ├── composition.log      #   aemeath:composition — composition 层
│   ├── agent-provider.log   #   aemeath:agent:provider — provider + LLM 输入/输出
│   ├── agent-runtime.log    #   aemeath:agent:runtime — agent 循环
│   ├── agent-tools.log      #   aemeath:agent:tools — tool 执行
│   ├── agent-prompt.log     #   aemeath:agent:prompt — Guidance 系统
│   ├── agent-hook.log       #   aemeath:agent:hook — hook 执行
│   ├── agent-storage.log    #   aemeath:agent:storage — 持久化
│   ├── agent-project.log    #   aemeath:agent:project — worktree 管理
│   ├── agent-policy.log     #   aemeath:agent:policy — 权限评估
│   └── agent-audit.log      #   aemeath:agent:audit — 审计事件
> **废弃文件**：`input.log` / `output.log` / `tool.log` / `audit.log` / `agent.log` 已废弃，NEVER 再使用。详见 `specs/logging.md`。
>
> **终端测试**：`echo '{prompt}' | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv`（`-q` 静默模式 + `-v` 日志输出到 stderr，适合非交互式 CLI 测试；`AEMEATH_VERSION=` 覆盖版本号、`RUST_LOG=` 覆盖日志级别，方便测试）。可用 `--model Zhipu/glm-5.2` 等参数指定模型。
>
> **日志级别**：全局配置 `logging.level` 默认 `info`，debug 级别日志仅在文件和 stderr 中可见（文件已路由到对应 target 文件）。调试时用 `AEMEATH_LOG_LEVEL=debug` 环境变量全局拉高到 debug：`AEMEATH_LOG_LEVEL=debug cargo run`；也可用 `RUST_LOG` 按 target 单独拉高：`RUST_LOG=aemeath:tui=debug,aemeath:agent:runtime=debug cargo run`。
>
> **注意**：`log::set_max_level()` 设为 `Info` 时会全局过滤掉所有 debug 日志。若需在非交互模式下验证 debug 日志，需将全局配置 `logging.level` 改为 `debug`，或将日志临时改为 `info!` 级别。
├── memory/                  # 持久化记忆存储
├── sessions/                # 会话持久化
├── skills/                  # 全局 Skills
├── mcp.json                 # MCP 工具配置
├── history.json               # 用户输入历史
├── cost_history.json          # 成本追踪历史
└── settings.json              # 全局设置
```

## 工作流约束

### Bug / Feature 执行流程

bug / feature 追踪改在 GitHub Issues（仓库 `rushsinging/aemeath`），按以下步骤执行，**NEVER** 跳过：

新建 issue 时 **SHOULD** 使用 `.github/ISSUE_TEMPLATE/bug.yml` 或 `feature.yml`，以保证 `kind:*`、`area:*`、`priority:*` 标签由 `.github/workflows/auto-labeler.yml` 一致地应用。创建 PR 时 **SHOULD** 使用 `.github/pull_request_template.md` 填写 Summary、Refs、Breaking change、Test plan。

1. **阅读 Issue**：用 `gh issue view <编号> --repo rushsinging/aemeath` 拉取 issue 标题、labels、完整 body（body 顶部有 `<!-- Migrated from: <source> -->` 标记，可追溯到原 `docs/bug/archived/<id>-<slug>.md` 或 `docs/active.md#<id>`）。设计稿类 issue **SHOULD** 配套阅读 `docs/snapshot/specs/<file>.md`，每份 spec 顶部已写入 `> 对应 Issue: <url>` 指针。
   - **MUST** 检查 issue 是否关联 milestone。未关联 milestone 的 issue **MUST** 先提醒用户关联，**NEVER** 在无 milestone 的情况下直接开 worktree 修改。
   - **MUST** 根据 milestone 版本号定位对应的 `release/vX.Y.Z` 集成分支（见下方「Git 工作流」）。分支不存在时 **MUST** 提醒用户从 `origin/main` 创建并 push。
2. **定位问题并给出方案**：阅读相关源码，定位根因或设计点，**MUST** 向用户输出可执行的修复/实现方案（含改动范围、根因分析、验证计划）。方案中的任务必须拆分为单一、具体、可验证的最小步骤（如“修改 A 文件的 B 函数”“为 C 场景添加测试”“运行 cargo clippy”），**NEVER** 使用“实施并验证”这类宽泛任务概括多个步骤。复杂改动 **MUST** 调用 `superpowers:writing-plans` 制定详细计划；即使是简单改动，也 **MUST** 先给出简明方案，禁止直接开始修改。
3. **等待用户明确同意**：在获得用户的明确书面同意（如“同意”、“开始改”）前，**NEVER** 调用 Edit/Write/Bash 等会修改文件或系统状态的工具。如果用户只给出笼统意图（如“修一下”）而未确认具体方案，**MUST** 先呈现方案并等待确认。
4. **执行与验证**：在 worktree 中实施，worktree **MUST** 基于 issue 所属 milestone 的 `release/vX.Y.Z` 集成分支创建（**NEVER** 直接基于 `main` 开 feature 分支，除非是无 milestone 的紧急修复且经用户确认）。通过编译、测试、clippy 验证后 PR 合入对应 release 分支。
5. **用户确认后关闭 Issue**：agent **NEVER** 自行关闭 issue。合并完成后，由用户确认是否关闭；用户确认后，可用 `gh issue close <编号> --repo rushsinging/aemeath --comment "..."` 关闭 issue，comment 引用合并 commit / PR。

修复 bug 或实现 feature 时，**MUST** 做根因层面的修正（fact-check），而不是只做最小化补丁绕过症状。应评估相关代码路径、数据流和状态机，确保同类问题不再复发。

如果同一问题既可临时止血也能彻底重构，**MUST** 同时给出最小化补丁和根因级彻底方案，并说明两者优劣、成本与风险。对于存在复发风险、明显设计缺陷或结构性不合理的情况，**SHOULD** 默认推荐并优先实施彻底方案，除非用户明确要求只做最小修改。

标签约定：
- `kind:*`：`kind:bug`（缺陷）、`kind:feature`（功能）、`kind:rfc`（重大设计问题）。创建 issue 时 **MUST** 二选一；涉及重大设计时 **SHOULD** 将 `kind:feature` 重标为 `kind:rfc`。
- `area:*`：根据改动路径自动标注（映射见 `.github/area-map.json`），多 area 改动可携带多个 `area:*` 标签。
- `priority:*`：`priority:high`、`priority:medium`、`priority:low`，有明确优先级时 **SHOULD** 添加。
- `breaking`：PR 标题含 `!` 或 body 含 `BREAKING CHANGE:` 时由 auto-labeler 自动添加。
- `migrated-from:docs` 仅用于历史迁移条目。

### Milestone / Release Gate 管理

Milestone 跟 release 版本走，用于表达某个版本要交付的**可验收能力包**，**NEVER** 作为 issue 分类桶或按子系统碎片化拆分。

1. **命名规则**：milestone 标题 **MUST** 使用 `vX.Y.Z — 能力目标`，版本号从现有 release 顺序继续递增；能力目标描述用户或系统能获得的可感知能力。
2. **范围规则**：每个 issue **SHOULD** 只归属一个 milestone；跨版本或长期方向的 RFC / backlog **SHOULD NOT** 进入 milestone，除非该版本明确要落地其 MVP。
3. **Release Gate issue**：每个 milestone **MUST** 有且只有一个验收 issue，标题格式为 `[Release Gate] vX.Y.Z — 能力目标`，并关联到该 milestone。
4. **必有 issue 类型**：除功能 / bug 执行 issue 外，每个 milestone **MUST** 包含以下三类 issue：
   - **Release Gate issue**（本条第 3 项）：版本验收单一真相。
   - **收尾退役 issue**：清理全项目的死代码、过期兼容层与旧路径（不限于本 milestone 引入）；去除 `cargo clippy --workspace --all-targets` 全部 warning。
   - **大文件拆分 issue**：对 milestone 改动涉及的核心大文件进行模块边界整理，确保单文件职责清晰。
5. **关联规则**：纳入版本范围的执行 issue **MUST** 设置同一个 milestone，并在 Release Gate issue 的关联清单中出现；范围变化时 **MUST** 同步更新 milestone 与 Release Gate issue。
6. **验收含义**：关闭 Release Gate issue 表示该 milestone 已满足发版判断，但 **NEVER** 替代“发版”流程；创建 / push release tag 仍 **MUST** 遵守下方发版规则并等待用户明确确认。
7. **进度维护**：执行 issue 合入、移出或发现阻断时，**MUST** 更新 Release Gate issue 的 checklist / 阻断项 / out-of-scope，保持它作为该版本范围与验收状态的单一真相。
8. **关闭关联 issue 时的 Release Gate 同步**：关闭任意关联了 milestone 的执行 issue 前，**MUST** 检查并更新对应 Release Gate issue：
   - 在「关联 issue」清单中标记该 issue 为已完成（`[x]`），若 issue 被移出 milestone 则注明原因与去向。
   - 若该 issue 引入了新的可验收行为、测试场景或手动验收步骤，**MUST** 补充到 Release Gate issue 的「验收标准」或「测试细节」中。
   - 若关闭后发现该 issue 的实际交付范围与 Release Gate 原定义不一致，**MUST** 同步调整 Release Gate 的 Must have / Should have / Out of scope。

Release Gate issue 模板：

```md
## 版本目标

用 1-3 句话描述这个版本交付的可感知能力，以及为什么该版本值得发布。

## 范围

### Must have

- [ ] 版本发布前必须完成的能力或修复。

### Should have

- [ ] 有价值但不阻断发布的能力；若延期，必须移出 milestone 或记录原因。

### Out of scope

- 明确不属于本版本的方向、RFC、重构或长期探索。

## 关联 issue

- [ ] #xxx
- [ ] #yyy

## 阻断项

- 当前无；如发现发布阻断，记录 issue / PR / 验证失败链接。

## 验收标准

- [ ] 所有 Must have 对应 issue 已关闭，或经用户确认移出 milestone。
- [ ] 必要的手动验收场景已执行并记录结论。
- [ ] `cargo test` 通过。
- [ ] `cargo clippy` 通过。
- [ ] release 前确认无阻断 issue。

## 发布判断

所有 Must have 完成且验证通过后，关闭本 issue，允许进入 `vX.Y.Z` 发版流程。
```

### 大型工作的拆分与跟踪（总 → 分 / 伞 issue）

跨多个子系统、需多个 PR 才能完成的大型工作，**MUST** 按"总 → 分"组织，**NEVER** 塞进单一 issue 或单一 PR：

1. **建伞（umbrella）issue 放大纲**：承载整体设计大纲、范围边界、子任务清单；大纲指向对应设计文档（`docs/superpowers/specs/`）。伞 issue 是该工作范围的**单一真相**。
2. **拆子 issue**：每个可独立验证、独立 PR 的单元拆成一个子 issue；子 issue body **MUST** 链接回伞 issue，伞 issue **MUST** 列出全部子 issue。
3. **标注依赖与并行性**：伞 issue **MUST** 对每个子 issue 标明——**可否并行**、**被谁 block / block 谁**，并据此排执行顺序。
4. **跟进进度**：伞 issue 维护进度清单（每个子 issue：未开始 / 进行中 / 已合入）；状态变化时 **MUST** 同步更新伞 issue。
5. **范围调整同步**：子任务执行中若发现更根本的问题需重做或移动范围，**MUST** 在伞 issue 与相关子 issue 同步调整（移入 / 移出 / 新建），保持伞 issue 大纲与依赖图始终为真。
6. **子 issue 拆分原则**：大型工作的子 issue **SHOULD** 不超过 7 个，**MUST** 包含以下两类收尾 issue：
   - **Guard + Verify issue**：落地架构守卫锁定新边界，故意制造违规验证拦截生效；端到端验收测试覆盖核心场景。
    - **收尾退役 issue**：清理全项目的旧路径、散点读取和死代码（不限于本次改动引入）；去除 `cargo clippy --workspace --all-targets` 全部 warning；更新 `specs/` 分片和 `docs/design/`。
    - **大文件拆分 issue**：对本次改动涉及的核心大文件进行模块边界整理，确保每个文件只承担单一职责。
   其余子 issue 按依赖层次（领域模型 → 适配器 → 消费方）自然拆分，每个子 issue **MUST** 可独立 PR、独立验证。依赖方向严格从内到外，**NEVER** 反向。

### Git 工作流

milestone 开始时从 `origin/main` 最新 commit 切出 `release/vX.Y.Z` 集成分支，作为该 milestone 所有 feature / bugfix 的开发主线。各 release 分支互相独立、按版本号递进。

- **MUST** 所有 feature / bugfix 在独立 worktree 中开发，worktree 基于 issue 所属 milestone 的 `release/vX.Y.Z` 分支创建；**NEVER** 直接 push 到 `release/*` 或 `main`（均受保护，只接受 PR）。
- **MUST** feature / bugfix 分支完成后通过 **Pull Request** 提交到对应的 `release/vX.Y.Z` 分支（base: `release/vX.Y.Z`）。
- **MUST** 合并 PR 时使用 **Squash merge**，将分支上的多个提交压缩为一个提交后合入；**NEVER** 使用 rebase merge 或普通 merge commit。
- **MUST NEVER** 由 agent 自动合并 PR。PR 创建后由用户 review，用户确认后手动合并；agent 只能在用户明确授权后执行合并动作。
- **MUST** 创建 PR 前，在 worktree 分支上执行 `git pull origin release/vX.Y.Z` 拉取最新集成分支；若存在冲突，解决后重新通过验证门禁，才能推送分支并创建 PR。
- **MUST** 同一 milestone 的所有执行 issue 合入 release 分支后，将 `release/vX.Y.Z` 通过 PR 合入 `main`（base: `main`，head: `release/vX.Y.Z`），squash merge 后在 main HEAD 打 `vX.Y.Z` tag 触发发版 workflow。
- 补丁版本（`X.Y.Z+n`）：在原 `release/vX.Y.Z` 分支上继续修复，或从 `origin/main` 新切 `release/vX.Y.(Z+1)` 分支；修复合入 main 后打 `vX.Y.(Z+1)` tag 发版。
- **MUST** 所有代码改动通过 release 分支合入 main，**NEVER** 直接向 `main` 开 PR。横切修复（clippy、格式、工具链升级、依赖 bump）也 **MUST** 落到当前活跃的 `release/vX.Y.Z` 分支，通过 release→main PR 传播。唯一例外：无 milestone 的紧急热修复，且经用户确认。

### 代码修改后检查

每次完成代码修改后（含 bug 修复、feature 实现、重构），**SHOULD** 检查是否产生了应当被移除的旧代码、废弃路径、过期兼容层、仅被测试引用的死代码，或已被新实现取代的临时方案。

发现上述情况时，**MUST** 向用户报告：问题位置、成因、未清理的风险，以及建议的清理方案（清理范围、是否在本次 PR 内处理、是否需要另开 issue 追踪）。**NEVER** 在知情的情况下让待退役代码静默遗留，除非用户明确接受并记录退役计划。

### 发版

发版通过 push `v*` tag 触发 `.github/workflows/release.yml` 自动完成（校验 → macOS 双架构构建 → 创建 GitHub Release + checksums）。**MUST** 遵守：

- **MUST** 由用户明确指定版本号（如"发 0.0.2"），agent **NEVER** 自行决定发版或推演版本号。
- **MUST** tag 打在 `release/vX.Y.Z` 合入 `main` 之后的 `origin/main` HEAD 上（已 review、已同步到远端），**NEVER** 打在本地未 push 的 HEAD 上。本地若有未 push 的 commit，**MUST** 先判断其是否为已通过 PR 合入内容的噪音（如 merge commit、仅含 `.agents/*` 等运行时副产物的提交）；若是则 tag 仍打在 `origin/main`，发版后 reset 清理。
- **MUST** 使用轻量 tag（沿用 v0.0.1 风格），格式 `vX.Y.Z`。**NEVER** 改 `Cargo.toml` 的 `workspace.version`（占位符 `0.0.0`）；实际版本由 `release.yml` 的 `build` job 显式注入 `AEMEATH_VERSION` env（取自 `validate.outputs.version`），`build.rs` 不再从 git 历史读 tag。
- **MUST** push tag 前先向用户输出方案（tag 指向哪个 commit、版本号、自上一 tag 以来的变更摘要）并等待确认；**NEVER** 未经确认直接 push tag。
- **MUST** push 后用 `gh run list --workflow=release.yml` 监控三个阶段（Validate & Gate / Build ×2 / Create Release）全部通过；任一失败 **MUST** 排查并报告。
- release notes 由 `generate_release_notes: true` 自动从 PR 生成，agent **NEVER** 手写发版说明。
- **MUST** 用 `gh release view vX.Y.Z` 确认 Release 已发布且包含 aarch64 / x86_64 tar.gz + checksums.txt 三个 asset。

### Hook 阻断处理

工作中若遇到 hook 阻断（例如 PreToolUse 阻止 Edit/Write）：

1. **MUST** 先止血：立即切换到正确的工作上下文（如进入 git worktree），让用户请求的原始操作能够继续执行。
2. **MUST** 向用户报告：发生了什么阻断、阻断原因、以及采取了什么措施来处理。

## 开放决策

- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）。
- Skill 系统的热重载策略。
- 多模型 pool 的自动故障转移策略。
- Server Foundation MVP 的落地路径（目前仅设计文档，尚无 `server` crate）。**server 代码落地时 MUST 同步在触发表补 `agent/features/server/**` 行并新增对应分片**，否则破坏渐进式披露的路径覆盖。
