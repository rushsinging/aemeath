# Harness² 调研报告

> 调研日期：2026-10-21
> 调研对象：[google-research/harness2](https://github.com/google-research/harness2)（Apache 2.0）与论文 [Harness²: Recursive Agent Harnessing for an Open World, arXiv:2610.14224](https://arxiv.org/abs/2610.14224)（2026-10-09，Google Cloud AI Research + Northwestern University）。
> 证据边界：本报告基于 README、论文摘要、源码（`core/prompts.py`、`core/tree.py`、`core/spec.py`、仓库文件树）与第三方评述（NYU Shanghai RITS）；未实际运行 harness2，未复现 benchmark 数字。

## 1. 结论

Harness² 是一个"模型权重冻结、只优化 harness"的递归自我改进（RSI）框架，面向开放世界任务（测试时遇到设计期未见的任务、无 ground-truth verifier）。其证据要点：

- **收益实测**：单步改进即可让 GLM-5 在 WorkBuddy-Bench 上 +11.5 分（64.2 → 75.7），超过 Gemini 3.5 Flash 基线（72.4）——更好的 harness 可以比更好的模型更值钱；弱模型获益更大。
- **方法核心**：两层递归（任务级 Propose–Probe–Compose 对比选择 + 改进级经验库复利），编辑面是 8 个可编辑组件构成的 harness 树。
- **对 aemeath 的意义**：组件层 aemeath 基本已有对应物且部分更强；真正的缺口是"把 harness 当可优化对象"的四层机制（统一组件树、任务级递归、改进级递归、调度评测面）。
- **主要代价与风险**：递归显著抬高交付前 token 消耗；执行器网络不受限、Codex 沙箱禁用，评测隔离需自行配置；"自我改进"实质是配置与指令文件的进化，不改权重。

## 2. 方法：两层递归

### 2.1 Task-level recursion（任务级）

对比式 **Propose–Probe–Compose**，无需标准答案反馈：

1. **Propose**：harness editor 对当前任务提出两类编辑——不变式编辑（invariant，跨任务通用）与领域编辑（domain-specific）。
2. **Probe**：候选 harness 在**同一任务**上实际执行，采集行为证据。
3. **Compose**：harness composer 从多次执行的行为**对比**中筛选真正有效的改动，合成精炼 harness。

两种扩展模式：

| 模式 | 行为 |
|---|---|
| parallel | 同时探测 K 个通用 + K 个领域候选，从共享证据合成 K 个组合（探索更宽） |
| sequential | 带着精炼 harness 与累积证据连续走 K 步（打磨更深） |

### 2.2 Harnessing-level recursion（改进级）

experience reflector 从每步采纳的 harness 与**证据支持的 lesson** 中提取经验，更新持久的 improvement harness（经验库），指导未来任务改进，使收益跨任务流复利。消融显示去掉持久经验库损失 3.1 分（WorkBuddy）/ 1.8 分（JobBench）；JobBench 积累的经验库能提升 WorkBench 单步改进效果，证明经验可迁移。

## 3. 8 个可编辑组件（源码 c1..c8）

以 OpenCode surface 为准（Codex surface 为参数化变体：单文件 `AGENTS.md` 承担 c1–c3、`hooks.json` 承担 c8）：

| # | 路径 | 是什么 | 何时进入上下文 |
|---|---|---|---|
| c1 | `systemprompt.md` | 角色与工作流程 | ALWAYS ON——每个 token 与 agent 输出预算竞争 |
| c2 | `guardrails.md` | 常驻规则 | ALWAYS ON |
| c3 | `LongTermMEMORY.md` | 持久领域事实 | ALWAYS ON |
| c4 | `tool_descriptions/tool_guidance.md` | 工具使用指南 | ALWAYS ON |
| c5 | `skills/<name>/SKILL.md` | 可复用工作流 | 按需加载，仅被调用时 |
| c6 | `sub_agents/<name>/AGENT.md` | 委派子代理 | 独立上下文运行，返回文本 |
| c7 | `scripts/<name>.py` | 确定性辅助脚本 | 按需经 bash 执行 |
| c8 | `plugin/<name>.ts` | 插件 | 常驻注册，作用于 tool loop 内部 |

源码（`core/prompts.py`）中三条关键设计契约：

1. **成本经济学决定形式选择**，优先级为 SKILL（默认形式，有可识别触发器）→ SCRIPT（确定性操作）→ SUBAGENT（"输入大、答案小"的有界子任务才划算）→ PLUGIN（loop 内机械问题）→ ALWAYS-ON 文本垫底，"加了就要减别的来付账"。最贵失败模式是"推理到预算耗尽什么都没产出"，加 prose 会让它更可能。
2. **PLUGIN 是唯一模型跳不过的组件**：prose 可以不被读，hook 不能。四个典型形态：裁剪噪声/超长工具输出、按错误签名追加 hint、限制失控工具、写后即验证。
3. **编辑契约 omitted-is-inherited**：提案只发变更项，未提及组件自动继承；校验失败整份提案作废（`core/tree.py` 提供 read/validate/write/diff，按 SurfaceSpec 参数化，两 substrate 共享同一逻辑）。

## 4. 结果

三个专业 benchmark、四种 model-harness 配置：

| 配置 | Benchmark | 单步提升 |
|---|---|---|
| GLM-5 / OpenCode | WorkBuddy-Bench | 64.2 → 75.7（+11.5，最大单步增益） |
| Gemini 3.5 Flash / OpenCode | WorkBuddy-Bench | 72.4 → 77.5 |
| Gemini 3.5 Flash | JobBench | 31.5 → 36.0（+4.5） |
| GLM-5 | LAB（Harvey 法律，per-criterion） | 59.6 → 71.4（+11.8pp） |

- k=5 并行递归再增 2.6–6.7 分，随递归预算单调上升。
- 消融（WorkBuddy / JobBench）：去持久经验库 −3.1 / −1.8；去双 editor（invariant+domain）−2.9 / −3.7；去 composition −1.6 / −1.9。

## 5. 代码结构（Python 包，无预置权重）

```
harness2/
├── core/        # proposer、recursion、library（经验库）、judge、ledger、
│                #   selection、tree（c1..c8 组件树）、spec、schedule、prompts
├── substrate/   # 执行器抽象：base / opencode（默认）/ codex（--substrate cx）
├── benchmarks/  # jobbench、lab、workbuddy 适配 + 任务渲染
└── cli/         # setup / check / judge / report / demo / run_sweep
third_party/     # 三家 benchmark runner（jb/lab/wb）+ patches
```

工程要点：Linux + Python 3.12 + uv；`harness2-demo` 本地演示无需模型凭证（确定性 editor + 真实 Python 执行）；跑分需 Node.js 20+、gcloud/Vertex AI（LAB 需 Pandoc/Poppler，WorkBuddy 需 Docker）；结果落 `runs/` 可断点续跑。

## 6. 限制与批评

- **代价**：递归显著抬高交付前 token 消耗；收益不均匀——Codex + Gemini 3.5 Flash 的 LAB 上仍落后于离线基线 Meta-Harness。
- **"自我改进"的实质**（NYU RITS 评述）：不改权重、不生成训练数据、不训 reward model、不自我博弈；改进的只是配置与指令文件，靠同任务执行对比筛选。是"文件层面的进化"。
- **评测隔离风险**：执行器网络访问不受限、Codex 沙箱禁用，公开 benchmark 评分标准可能被在线访问；评测前必须自行配置网络限制，否则结果不可信。

## 7. 与 aemeath 的映射与吸收方案

### 7.1 组件层现状映射

| Harness² 组件 | aemeath 对应物 | 差距 |
|---|---|---|
| c1 systemprompt | prompt feature（STATIC_SYSTEM_PROMPT + guidance） | 已有 |
| c2 guardrails | policy + hooks | 已有 |
| c3 memory | memory + reflect + 三条注入通道 | 强于其单文件形态 |
| c4 tool_guidance | tools feature 工具描述 | 已有 |
| c5 skills | skill 系统（bundled + 目录加载） | 已有 |
| c6 sub_agents | AgentTool 子代理系统 | 已有 |
| c7 scripts | 无一等组件，靠 Bash 现场写 | 缺落盘位 + 校验 |
| c8 plugin | hooks + MCP 工具 | 能力等价，缺统一抽象 |

### 7.2 真正的缺口：harness 当可优化对象的四层机制

1. **统一 Harness 树抽象**：c1..c8 的 read / validate / write / diff + omitted-is-inherited 编辑契约；aemeath 组件分散在各 feature，无单一可编辑面。
2. **Task-level recursion（PPC）**：invariant/domain 两轴提案 → 同任务多 rollout 探测 → 行为对比合成；aemeath 完全没有。
3. **Harnessing-level recursion**：experience reflector 把证据绑定 lesson 写入持久 improvement harness 跨任务复利。与现有 memory reflect 语义不同（记忆治理 vs harness 进化）。
4. **递归调度与评测面**：--k / parallel|sequential、ledger、selection、judge/report。

### 7.3 关键决策点（待拍板）

1. **交互式 vs 离线**：Harness² 每任务探测 2k 个 rollout，成本倍增；aemeath 是交互式 TUI，倾向默认后台/离线改进（复用 reflect 的 detached 派发骨架），交互会话可选轻量单步。
2. **组件面范围**：8 组件全量（含 scripts/plugin 落盘位与校验器），还是首批 6 个（c1–c6）。
3. **经验库复用边界**：lesson 的派发/落盘/写锁复用 reflect 基础设施，schema 与采纳门禁（证据绑定）独立。
4. **评测面**：aemeath 自带 judge/report，还是只做改进循环、评测外置。

### 7.4 交付拆分建议（0-based，共 5 档）

| 档 | 内容 | 验收 |
|---|---|---|
| PR0 | 设计文档 + 组件树契约 | 用户审设计 |
| PR1 | Harness 树抽象 + c1–c8 落盘/校验/diff | 单测 + 守卫 |
| PR2 | PPC 任务级递归（两轴提案 + 探测 + 合成） | L4/L5 + 真模型 e2e |
| PR3 | 改进级递归 + 经验库（证据绑定门禁） | L4/L5 + 真模型 e2e |
| PR4 | 调度面（--k/mode）+ CLI/TUI 入口 | 手动验收 |

## 8. 参考

- 仓库：https://github.com/google-research/harness2
- 论文：https://arxiv.org/abs/2610.14224
- 第三方评述：NYU Shanghai RITS, "Google Open Sources Harness2 for Self-Improving AI Agents"（2026-10-19）
