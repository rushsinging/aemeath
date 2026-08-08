# 08 · Catalog 与 Config 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | Config、Workspace、Skill/Model/Session catalog 对应 owner |
| Authority | 当前配置、workspace 或目录事实；不驱动 Run terminal |
| Identity | config revision、workspace revision、catalog revision、session identity |
| Ordering | revision 或 atomic full snapshot |
| Delivery | full-state、snapshot、typed command result |

本事件族不得退化为 `*Info` / `*Data` 的无 owner 消息集合。

## 2. Config

| Runtime / control-plane | SDK | TUI | Consumer | 状态 |
|---|---|---|---|---|
| `ConfigChanged` | 同名 | 同名 | 原子替换 config view | Current |
| `ConfigReloaded` | 同名 | 同名 | 应用 reload 后完整 config view | Current；需区分 Changed/Reloaded 职责 |
| `ThinkingChanged` | 同名 | 同名 | Config + visible presentation | Target Rename：`ReasoningChanged` 候选 |
| `ModelSwitched` | 同名 | 同名 | 更新 current model | Target Rename：`ModelChanged` 或 command ACK/事实拆分 |
| `ContextEstimated` | 同名 | 同名 | 显示 context estimate | Target Rename：需明确 Subject 与 Changed/Snapshot |

`ModelSwitched` 可能混合 command result 与 current config fact。后续迁移 MUST 先拆清 ACK 与 state fact，禁止只做机械 rename。

## 3. Workspace

| Runtime | SDK | TUI | Consumer | 状态 |
|---|---|---|---|---|
| `WorkingDirectoryChanged` | 同名 | `WorkspaceSnapshot` | `WorkspaceIntent::ApplySnapshot` | Target Rename：`WorkspaceChanged` |
| `ProjectInfo` | 同名 | 同名 | App-level project projection | Target Rename：`ProjectSnapshot` 或并入 Workspace |

Workspace payload 应包含 path base、workspace root、branch、worktree kind 与 revision。TUI 不自行调用 Git/config reader 重建 Runtime-owned workspace fact。

## 4. Catalog

| Runtime | SDK | TUI | Delivery | 状态 |
|---|---|---|---|---|
| `SkillsUpdated` | 同名 | 同名 | revisioned full catalog | Target Rename：`SkillCatalogChanged` |
| `ModelList` | 同名 | 同名 | full model catalog | Target Rename：`ModelCatalogChanged` / `Snapshot` |
| `ReminderList` | 同名 | 同名 | full reminder collection | Target Rename：`ReminderCatalogChanged` |
| `SessionList` | 同名 | 同名 | full session catalog | Target Rename：`SessionCatalogChanged` |

复数名词 + `Updated/List` 不表达 Subject 和 delivery。迁移时 MUST 明确这些 payload 是 revisioned Changed 还是 query response Snapshot。

## 5. Command/query result

| Runtime/SDK | TUI | 状态 |
|---|---|---|
| `CommandResultText` | system/error presentation | Compatibility；stringly result |
| `Result` | 映射为 command result text | Deprecated compatibility |
| `ReflectionHistory` | safe history view | Target Rename：`ReflectionHistorySnapshot` |
| `CostUpdate` | cost presentation | Target Rename：`CostChanged`，并声明 delta/cumulative |

新 command SHOULD 返回 typed outcome 或发布明确 Subject fact，禁止扩展 `CommandResultText` 成通用业务通道。

## 6. Atomic replacement

Catalog/Config full-state 事件 MUST：

1. 同一 revision 一次替换完整集合；
2. 空集合删除旧条目，不解释为“无变化”；
3. stale revision 丢弃；
4. 不分别 merge metadata 与 route 等相互一致字段；
5. query response 与 unsolicited changed event 在名称或 delivery 字段上可区分；
6. 不从 UI 当前显示值推断 authoritative config。

## 7. 与 Published State 的边界

- Runtime Status MAY 引用 current/pending config 和 workspace facts，但 Registry 不能每个 heartbeat 临时跨 BC 查询拼装；
- Config/Catalog owner 先发布 committed fact，Published State observer 再更新 full-state；
- 单独 catalog event 可服务专用列表 UI，不等于 Runtime Status family；
- 两者若并存，必须共享同一事实版本或明确独立 revision，不允许竞态双真相。

## 8. 变更门禁

修改 Catalog/Config event 必须说明 owner、revision、full-state/response 语义，更新 SDK DTO/schema、TUI atomic replacement、空集合/stale 测试和 [09-event-index.md](09-event-index.md)。
