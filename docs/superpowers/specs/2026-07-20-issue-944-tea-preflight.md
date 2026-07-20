# Issue #944 开发前门禁记录

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)，父 Issue：#860。
> 基线：`origin/main` `89ac5d7e8451de4cdf65c280613603e300a97149`。
> 本文只记录 Current → Target 差异、责任划分与执行门禁；Target 语义以设计文档为准。

## 已核对的 Target 文档与契约

- `docs/design/01-system/03-context-map.md`：TUI 是 Runtime 的入站适配器，只经 AgentClient / SDK Published Language 交互。
- `docs/design/02-modules/tui/01-architecture-and-dataflow.md`：唯一事件链、六 Context、Msg / Intent / Change / Effect 以及 root reducer 唯一写入口。
- `docs/design/02-modules/tui/02-model.md`：RunProjection、InteractionState、互补投影、Workspace revision 和 ViewState 的所有权。
- `docs/design/02-modules/tui/03-event-flow-and-acl.md`：#943 提供的 TUI-owned DTO 与 #944 的 Intent / Effect / result Intent 消费边界。
- `docs/design/02-modules/tui/04-view-layer.md`：ViewAssembler → ViewModel → Render 单向边界。
- `docs/design/03-engineering/03-migration-governance.md` O6 / TUI-2～TUI-7：#944 与 #943/#947/#878/#1246 的责任。
- `docs/design/03-engineering/04-testing-and-coverage.md`：L0 Guard、L1 状态机、L2 reducer/effect、L3 interaction 契约、L4 suspension / cancellation / stale-revision 场景。

## 原生依赖门禁

| Issue | 状态 | 结论 |
|---|---|---|
| #943 | Open，现被 #944 阻塞 | #944 先建立 TUI-owned Intent / Model / reducer 消费面；#943 随后接入第一层 SDK→TUI DTO ACL。 |
| #878 | Open | Runtime waiter / continuation 生产切线未完成；#944 可先实现 TUI 纯投影与 command result 消费，不假定 production sender 已删除。 |
| #1246 | Open，blocked by #944/#943 | Main suspension 生产切线等待本 Issue的 TUI state/effect 契约。 |
| #945 / #742 | Open，blocked by #944 | 本 Issue只提供 Cancel / RunProjection 所需的纯 State、Change 和 Effect 边界，后续 leaf 实现专门行为。 |
| #947 | Open，blocked by #944/#1246/#946 | 本 Issue不删除 legacy `update_ui`、sender、同步 git 或 App 双路径。 |

## 文档—代码差异

| Target | Current 证据 | #944 处置 | 明确承接 |
|---|---|---|---|
| 六 Context 私有并经 root reducer 唯一写入 | `TuiModel` 只有公开 `conversation/diagnostic/input/session`；Config / Workspace 混入 Conversation runtime；41 处非 reducer `apply()` | 建立六 Context 根结构、访问器、统一 `AgentIntent` 和 reducer dispatch；测试/构造仅走明确 test fixture | #947 删除剩余旧调用点与兼容字段 |
| ACL 只产 Intent，reducer只产 Change | `AgentEventMapping` 含 `effects`，`root_reducer` 直接透传；`update_ui` 在 reducer 后直接写 Model | 定义 `ModelChange` / `TuiUpdateResult` 仅表达 Change 与 dirty；Coordinator 从 Change 唯一导出 Effect | #943 清除 mapper SDK DTO；#947 删除 update_ui |
| Interaction 持 request id/body/draft/phase，TUI 无 sender | `AskUserBatch` 持 sender，InputState 亦持 reply sender；仅 legacy AskUser block | 定义 TUI-owned InteractionState、四 body typed draft、可恢复 phase 与 conflict；root reducer 只接受纯值 Intent | #943 转换 SDK request DTO；#947 删除 sender；#878/#1246 接 Runtime waiter |
| interaction result不改变Run | 旧 `update_ui` 直接 spinner / processing 操作；Run 状态为 chat-based模型 | 建立 RunProjection 状态变迁与 Interaction result Change 分离；只有 Run lifecycle Intent 改 Run 状态 | #943 传递 Run lifecycle；#945 取消 effect；#947 删除旧 spinner |
| Workspace snapshot / metadata revision 防陈旧 | `WorkingDirectoryChanged` 同步 git，Workspace 状态落 Conversation runtime | 设 WorkspaceProjection，snapshot apply 产生 metadata Effect；只在 root+revision 匹配时 apply metadata | #943 转纯 snapshot；#947 删除同步 git |
| ViewState只存瞬时交互态 | App/ViewState 含 spinner 镜像，Model 核心字段公开 | 将业务状态归 Model Change；ViewState仅维护 animation/scroll/selection/cache | #742 / #946 / #947 继续收口 |

## Guard 与白名单预算

| Guard | 当前基线 | #944 目标 | 允许保留 / 删除责任 |
|---|---:|---:|---|
| `check-tui-tea-purity.sh` migration exception | 1 | 1 | `app/slash.rs` 仍由 #947 处理；不新增 |
| `check-tui-effect-boundary.sh` path exception | 0 | 0 | 新 reducer / model 不得引入副作用 |
| `check-tui-model-view-boundaries.sh` explicit allowlist | 0 | 0 | 增加正向结构断言，不添加白名单 |
| `check-render-pure.sh` scope exclusion | 1 | 1 | display bridge 由 #947 删除 |
| inline TUI structure guard exclusion | 1 | 1 | 不增加 |

## 测试矩阵

| 行为 / 风险 | L0 | L1 | L2 | L3 | L4/L5 |
|---|---|---|---|---|---|
| 六 Context 私有与唯一 reducer 写入 | 静态 guard | accessor / private mutation | root reducer dispatch | — | 不需要 L5 |
| RunProjection 状态机 | — | 可达/非法转换 | Intent→Change | SDK lifecycle 字段由 #943 | cancellation 场景 |
| 四类 Interaction body / phase | sender-free guard | draft / phase 转换 | Intent→Change→Effect | request/reply body identity | #1246 suspension 场景 |
| Interaction outcome 不推进 Run | — | phase / run 不变量 | reducer + coordinator | AgentClient outcome mapping | reply / cancel journey |
| Workspace stale revision | — | tuple 匹配 | Change→metadata Effect→result Intent | snapshot shape 由 #943 | stale result 场景 |
| timeline/runs 互补投影 | — | invariant | reducer 原子事务 | — | output scenario |

## 结论

#944 现已满足启动条件。实现必须先以失败测试锁定 Context 私有性、Intent→Change→Effect 与 Interaction/Run 分离；每个阶段保持旧 App 路径可运行，但不得新增 sender、第二状态源或新的非 reducer mutation。旧路径的物理删除只能留给 #947。
