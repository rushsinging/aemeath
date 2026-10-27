# Harness · 组件接入契约与排期

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义各组件进入 registry 可编辑面的契约细节与优先级排期；依赖 #01（registry）与 #02（热载）先行。

## 1. 排期（按收益排序）

| 优先级 | 组件 | 理由 |
|---|---|---|
| P1 | c5 skills | harness2 成本经济学的默认形式；整文件可验证；调用时 `discover_all` 已天然热载 |
| P1 | c1 overlay（AGENTS.md 链路） | 已热载已 watch，增量最小 |
| P2 | c7 scripts（新增） | 天然热（执行时读盘），工程小 |
| P2 | c2 guardrails（新增） | 规则维度独立演化；RSI 编辑后置到热载稳定后 |
| P3 | c4 tool_guidance 覆盖层（新增） | 风险高，只做覆盖层 |
| P3 | c6 config 热载 | 支撑项 |
| 0.3.0+ | c8 plugin、多 rollout PPC | 见第 3 节 |

## 2. 组件契约细节

### c5 skills
- 读写单位：单个 `SKILL.md`（含 frontmatter）；NEVER 批量重写 skill 目录。
- validate：frontmatter `name` 与目录一致、`description` 非空（对齐现有加载器静默丢弃规则——registry 侧由静默变显式报错，供 RSI 修正）。
- 可见性：新 skill MUST 被引用（guardrails/tool_guidance/skill 自身 description 触发词）才对模型可用——对齐 harness2「named in always-on text」规则，防死件堆积。

### c1 overlay
- 只读聚合现有注入链路（全局 + 项目 5 级 + 安全检查）；registry `write` 仅开放**当前 cwd 解析出的最高优先级文件**（项目级），全局文件编辑后置。
- diff 语义：按注入顺序逐层 diff。

### c7 scripts（已退役，#2060 并入 c5）
- 独立组件退役：无独立注入通道（可见性寄生其他组件文本）不满足自洽四要件。
- 脚本回归 **skill 伴随资源**形态：`skills/<name>/scripts/`，SKILL.md 正文
  引用即对模型可见；执行仅经 Bash 工具，NEVER 新执行通道。
- 存量 `~/.agents/scripts/` 不迁移不删除（目录退役即不再扫描）。

### c2 guardrails
- always-on 注入段，进入 prompt 装配链；token 预算记账（加规则要付账，注入段有界）。
- 与 AGENTS.md 分层：AGENTS.md = 项目知识与约定；guardrails = 行为约束。

### c4 tool_guidance 覆盖层
- 形态二选一（开放决策 D1）：独立文件 vs 并入 guardrails 段（省 always-on 预算）。
- **注入通道**（D1 第二维度，NEVER 进工具 schema——外置 schema 会版本漂移破坏调用）：
  - 通道 A：system prompt stable 段尾（prompt caching 友好、每 run 必见；代价：修改使缓存失效）
  - 通道 B：user-context 消息（与 AGENTS.md 同通道；改动不破缓存）
- 注入内容：跨工具使用技巧 prose（工具选择、超时策略、分段读取、大结果处理）——RSI 证据最丰富（工具误用/重试信号）的槽位。

### c6 config
- `aemeath.json` 进 watch 面；变更走 run 边界生效 + Remind 通知（复用既有 reload 策略）。

## 3. 0.3.0+ 预告（本里程碑不实施）

- **c8 plugin**：tool loop 中间件 + TUI 渲染面（对齐 Claude mod 能力面：状态行/拦截 tool call；DeepSeek seam 架构：稳定干预点 + 可插拔 provider）。首批 seam 候选：裁剪超长工具输出、按错误签名追加 hint、工具限流、写后即验证。生效粒度：run 边界（loop 内不中途换）。
- **多 rollout PPC**：invariant/domain 双轴、parallel/sequential、--k 调度、ledger/judge 评测面。

## 4. 开放决策清单

| # | 决策 | 默认倾向 | 拍板时点 |
|---|---|---|---|
| D1 | c4 形态：独立文件 vs 并入 guardrails 段；**注入通道**：A（system prompt stable 段，prompt caching 友好但修改破缓存）vs B（user-context 消息，改动不破缓存、语义权重略低） | 并入段 + 通道 B | P3 实施前 |
| D2 | run 内 background process 跨边界的组件版本归属 | 按发起 run 快照 | 热载实施时 |
| D3 | overlay 全局文件是否开放 RSI 编辑 | 先只开放项目级 | P1 验收后 |
| D4 | 经验库 Active 容量与整理策略数值 | 100 条/超限 LLM 整理 | daemon 实施时 |

## 5. 验收汇总

各组件验收遵循统一分层（L2/L4/L5 + 真模型 e2e），矩阵见 02 分片第 4 节与 03 分片第 6 节；新组件（c2/c7/c4）额外要求：注入/可见性/校验器单测齐备后才进 RSI 可编辑面。
