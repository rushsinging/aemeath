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

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：八层 TEA 管线、六 Context 投影、SDK DTO 边界、架构门禁、reducer 纯化目标态 | #795 |
| 2026-07-12 | 新增 02-model：六 Context 完整字段、投影状态机、Model 纯净性约束 | #796 |
| 2026-07-12 | 新增 03-event-flow-and-acl：两层 ACL、六 Context Intent / Effect、agent_id / sub-agent 路由 | #797 |
| 2026-07-12 | 新增 04-view-layer / 05-e2e-scenario-testing | #795 |
| 2026-07-16 | 冻结 TUI Target 目录：八层 TEA 管线映射为 `adapter / model / update / effect / view_assembler / render` 六个技术目录，**NEVER** 采用 `capabilities/`；目录承载单向数据流与 import 隔离 | [#972](https://github.com/rushsinging/aemeath/issues/972) / [#991](https://github.com/rushsinging/aemeath/issues/991) |
