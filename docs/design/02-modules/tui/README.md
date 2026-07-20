# TUI · 模块总览

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [architecture-and-dataflow.md](01-architecture-and-dataflow.md) | 八层 TEA 管线、三条信息流、六个 Context / Projection、Msg / Intent / Change / Effect、ViewAssembler / ViewModel / ViewState、SDK DTO 边界与架构门禁 |
| 02 | [model.md](02-model.md) | Conversation / Input / Diagnostic / Session / Config / Workspace 私有核心投影、Runtime 权威状态机、runs / timeline 互补投影、四类 Interaction request-id 状态与 Model 纯净性约束 |
| 03 | [event-flow-and-acl.md](03-event-flow-and-acl.md) | 唯一 SDK event → TUI DTO → Intent → Change → Effect → result Intent 链、两层 ACL、六 Context 穷尽映射、Runtime-owned interaction id / AgentClient reply、agent_id / sub-agent 路由与门禁 |
| 04 | [view-layer.md](04-view-layer.md) | block 类型、ViewAssembler 组装、OutputViewCache memo、ViewState（滚动/选区/折叠/动画）、缓存、Render、选区复制与主题 |
| 05 | [e2e-scenario-testing.md](05-e2e-scenario-testing.md) | 基于 ratatui TestBackend、crossterm 与 insta 的进程内 E2E 场景测试边界、单帧驱动器、Harness、Effect Driver、确定性约束、快照治理、P0/P1 场景矩阵与 CI 门禁 |

## 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 Runtime-owned `AgentClient` 入站 OHS（由 SDK 发布契约）与 Runtime 通信；从 TUI 视角它是唯一对外依赖
- 不承载业务逻辑——纯展示层
- 基于 The Elm Architecture（TEA）变体
- `UiEvent` **NEVER** 直达 Model：所有 SDK 事件必须经两层 ACL、六 Context Intent、reducer Change、Coordinator Effect 与 result Intent 闭环
- UserQuestions、ToolApproval、PlanApproval、HardPause 共用 Runtime 生成的 Interaction request id，并经 SDK / TUI ACL / AgentClient reply command 无损贯穿；TUI **NEVER** 持有 sender、pending waiter 或自生成协议 id
- Interaction command result 只结束本地交互块；Run 只由 SDK `RunResumed` / `RunCancelling` / `RunCancelled` 等 Runtime 权威事件推进
- 六 Context 核心字段私有，root reducer 是唯一写入口；ViewAssembler 只读 accessor，ViewState 只持瞬时交互 / 渲染状态
- Conversation 的结构化投影（runs / queued / progress）与 `timeline` 是同一 reducer 事务原子维护的互补投影，只约束重叠事实，**NEVER** 假定可完整互相重建

### Reflection 展示边界（#899）

- Runtime 的 Interval / PreCompact / Manual Reflection 全部后台异步执行；完成时 **NEVER** 主动向 TUI 发送完整 `ReflectionResult`、formatted content、正文或完成通知块，TUI 不维护 reflection job 结果通道。
- `/reflect [limit]` 是只读命令：只向 Runtime 查询 Memory-owned durable history，默认/显式 limit 均返回 newest-first 的安全摘要；命令不触发 Reflection、不等待 Provider、也不 apply Memory。
- TUI 只能展示 SDK `ReflectionHistoryView` 中的 id、时间、trigger、status、deviation/suggestion/outdated 数量、apply status、error category、token usage 与 duration；**NEVER** 展示或记录 prompt、对话、Memory content、raw response、parsed output、formatted content 或正文截断。
- Reflection 的 busy skip、后台成功/失败、drain/cancel/timeout 属 Runtime 诊断与生命周期语义，除显式 history query 返回的安全记录外不进入 Conversation timeline。

## Target 目录结构

TUI 作为入站适配器与纯展示层，不采用 `capabilities/` 业务竖切，而采用与八层 TEA 管线一一对应的技术目录。判据与命名规则以 [代码组织规范 §3](../../01-system/06-code-organization.md) 为唯一真相源。

```text
tui.rs                 # 模块窄 façade：受控 re-export 与 App 入口
tui/
├── adapter/           # ① 终端事件 → TuiMsg、② SDK ChatEvent → UiEvent、ACL 转换
├── model/             # ④ TuiModel 根、六 Context、ViewState、OutputViewCache
├── update/            # ③ Coordinator、root reducer、Intent 拆分与 Change 归并
├── effect/            # ⑧ Effect enum、EffectDriver、result Intent 闭环
├── view_assembler/    # ⑤ OutputViewAssembler / StatusViewAssembler / InputViewAssembler / DialogViewAssembler → ViewModel
└── render/            # ⑦ ratatui Buffer 写入、主题、ViewState 与 OutputViewCache 应用
```

| 目录 | TEA 层级 | 内容 |
|---|---|---|
| `adapter/` | ①② | crossterm Event → `TuiMsg`、`sdk::ChatEvent` → `UiEvent`、六 Context ACL 入口；承载终端与 SDK wire type |
| `model/` | ④ | `TuiModel` 根、六 Context 投影、ViewState、OutputViewCache 等只读 / 派生数据；纯函数，不执行 IO |
| `update/` | ③ | `App::update`、Coordinator、`map_agent_event`、`root_reducer`、`model::apply`、`effects_for` 的组合 |
| `effect/` | ⑧ | `Effect` enum、EffectDriver、EffectExecutor、SDK reply、副作用与 result Intent 闭环 |
| `view_assembler/` | ⑤ | 四个 ViewAssembler 与其 ViewModel 产出；纯数据，不依赖 ratatui |
| `render/` | ⑦ | ratatui Buffer 写入、主题与 ANSI 处理、OutputViewCache 读、动画 |

ViewModel / ViewState 作为 `view_assembler/` 产出的纯数据值类型就近保留在该目录下，不单独成层；测试夹具、Effect Driver 与架构门禁就近归 `update/`、`effect/` 私有模块内文件，**NEVER** 单设横向 `tests/`、`fixtures/`、`drivers/`。

### 不采用 `capabilities/` 的判定

依据 [代码组织规范 §3](../../01-system/06-code-organization.md) 的递归能力判据与技术目录启用条件：

1. **TUI 是交付层而非 Bounded Context**：六 Context（Conversation / Input / Diagnostic / Session / Config / Workspace）都是同一 UI 状态在同一 Root Reducer 事务下的投影切面，共享 Coordinator 入口与 `TuiModel` 根所有权；它们不具备独立的稳定词汇、变化原因、状态所有权或独立测试夹具，无法构成 §3.1 的候选能力证据。
2. **目录服务于单向数据流**：`adapter → model → update → view_assembler → render` 与反馈环 `effect → model` 形成固定八层管线，每一层都是物理隔离点；按 TEA 步组织使依赖图自然沿正向管线外延，`render/` 仅依赖 `view_assembler/` 产出的纯 ViewModel，禁止反向从渲染层穿透 reducer 或 model。
3. **目录服务于 import 隔离**：`model/` 与 `effect/` 是稳定策略核心，`adapter/` 与 `render/` 是终端、ratatui 等易变技术 detail；按 TEA 步组织使得 ViewAssembler 与 Render 可独立替换为不同后端（如 `crossterm TestBackend` 或未来 headless renderer）而不影响 reducer 与 model。
4. **避免误植递归竖切判据**：六 Context 由 Coordinator 显式桥接，并不独立演进；若为对齐其他 BC 的可视化而预建 `capabilities/`，会稀释 TUI 的数据流边界并破坏 `update/` 对六 Context 的统一入口，违反 §3.1 "锁步变化保持同叶子" 的要求。

技术目录的引入与命名遵循 [代码组织规范 §3.7](../../01-system/06-code-organization.md)；TUI 不存在 provider / protocol 维度上的多种集成，因此 **NEVER** 在 `adapter/` 内嵌套跨 provider 子目录，也不为对称性预建 `ports/` 或 `views/` 等空层。

## 相关文档

- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 模块级 Target 目录结构矩阵：[../README.md](../README.md)

## 修改历史

> **#899 durable lifecycle / compact boundary:** accepted job 先 append `Running`，成功、失败、partial apply、timeout/cancel 均以同 id `upsert` 终态；cancel 不删除 durable fact。PreCompact 只在 compact 成功产生 outcome 后 submit 预先冻结的“将被丢弃”快照；compact 失败不 submit，busy 结构化 warn 后立即 skip，绝不排队。

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-20 | #1001 复核 capability-first 判据：TUI 作为交付层维持 TEA 技术目录，六 Context 不构成独立业务竖切；`tui.rs` façade 仅发布 `App`，render widget 保持内部实现细节；`app/`、`view_model/`、`view_state/` 的语义收敛继续由 #944/#947 承接 | [#1001](https://github.com/rushsinging/aemeath/issues/1001) |
| 2026-07-18 | #899 冻结 Reflection 静默后台语义：TUI 不接收主动结果；`/reflect [limit]` 只读展示 Memory history 安全摘要且日志不含正文 | #899 |
| 2026-07-12 | 初稿：八层 TEA 管线、六 Context 投影、SDK DTO 边界、架构门禁、reducer 纯化目标态 | #795 |
| 2026-07-12 | 新增 02-model：六 Context 完整字段、投影状态机、Model 纯净性约束 | #796 |
| 2026-07-12 | 新增 03-event-flow-and-acl：两层 ACL、六 Context Intent / Effect、agent_id / sub-agent 路由 | #797 |
| 2026-07-12 | 新增 04-view-layer / 05-e2e-scenario-testing | #795 |
| 2026-07-16 | 冻结 TUI Target 目录：八层 TEA 管线映射为 `adapter / model / update / effect / view_assembler / render` 六个技术目录，**NEVER** 采用 `capabilities/`；目录承载单向数据流与 import 隔离 | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
