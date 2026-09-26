# Published Language 重构原则

> 适用范围：各 feature crate 对外导出面（crate 根 `pub use` / `pub fn`）的收敛与治理。
> 实践来源：#1705 audit、#1710 policy、#1711 project、#1706 config（PR #1732）。
> 后续承接：#1707-#1716 各 crate issue 与 #1696 facade 消费收窄。

## 一、收敛 export

每个 crate 的每个导出必答四问：

| 问 | 判定 | 实例 |
|---|---|---|
| 生产零消费？ | **删**（测试一起删——测试不为死代码续命） | audit `summarize` 全链、policy `PolicyModeReader` |
| 纯转发 re-export？ | **删**（消费方直连定义 crate） | project 转发 share 的 `WorkspaceId`/`WorktreeKind` |
| 同构重复结构？ | **合并** | `WorkspaceWiring`/`WorkspaceViews` → `Workspace` |
| 跨 crate 仅测试直连？ | 判定记录，随测试迁移批收窄（#1696） | `ConfigAppService`（owner-test-consumed） |

## 二、export 命名规则化 + guard

### 封闭四类语法

导出符号只允许以下四种形态（封闭集，`facade.<crate>.root-exports` 守卫）：

| 形态 | 语义 | 实例 |
|---|---|---|
| `wire_*` | 工厂：装配入口（含 blob/存储适配选择） | `wire_production_workspace`、`wire_config_override_store` |
| `*Data` | 数据契约：跨 crate 载荷（含持久化 DTO） | `WorkspaceData`、`ConfigUpdateData` |
| `*<Role>` | 角色：人名化或显式名词；禁抽象概念词（Source/Sink/Port/Client） | `WorkspaceReader`、`AuditWriter`、`Policy` |
| `*Error` | 错误：跨界唯一为 `share::error::DomainError` | （crate 内细变体不算导出类） |

### 双 guard 守卫

- `facade.<crate>.root-exports`——白名单外的 crate 根导出即违规（防回潮）；
- `construction.cross-bc.fail-closed`——未登记的跨 BC `wire_*` 调用即违规（装配点收敛；新 wire 工厂须同步登记 `construction_symbols`，`kind: "wire"`）。

## 三、DDD 判据（Published Language 收录五类）

PL 只收 DDD 五类实体——**命令、事件、值对象、端口、工厂**。§二 的四类语法是命名形态（guard 可机检），五类是语义收录判据（评审判定）；一个根导出必须同时通过两层：形态合规，且属五类之一。五类之外的形态按下表降级处置：

| 非五类形态 | 判据 | 处置规则 | 判例（config 收敛） |
|---|---|---|---|
| 伪概念：以 `Err` / `Rejected` 变体伪装失败的"结果枚举" | 失败是控制流，不是领域状态 | Result 化：签名 `Result<T, DomainError>`；确属领域状态的分支保留为值对象/事件 | `ConfigRefreshOutcomeData` 只剩 `Unchanged`/`Reloaded` 真状态，伪 `Rejected` 消亡；`ConfigPersistOutcomeData` 随 persist 轨 Result 化删除 |
| 实现体：应用服务、adapter、具体存储/文件系统实现 | 消费方只见行为，不见类型 | 经工厂返回 trait 对象或句柄，实现类型 `pub(crate)` | `ConfigAppService`、`NativeConfigStore::new`、`FilesystemGlobalConfigConnectStore::new` 收窄 crate 内，跨界只经 `wire_*` |
| 表示模型：面向单一消费者的 UI / 序列化投影 | 为表示层服务，非共享协议 | 下沉子发布语言模块（模块=1 计数），不逐符号上根 | `form`（连接表单投影）、`catalog`（Provider 目录协议） |
| Saga / 多阶段编排状态 | 阶段由单 crate 编排，跨 crate 只消费入口与事件 | 整模块登记为子发布语言，或经端口/事件收窄 | `connect` 状态机整模块发布；prepare/commit 协议经 `ProjectConfigParticipant` 端口跨界 |

config 收敛为本批判例：根 PL 面按五类重组（命令 2 / 事件 4 / 值对象 7 / 端口 5 / 工厂 6），实现体与判定辅助（`ConfigAppService`、`is_persist_conflict`）进判定记录；`catalog`/`connect`/`form`/`ports` 登记为子发布语言模块（模块即边界，模块=1 计数），`runtime_resolution` 仅两个 free fn 跨界、`user_agent` 私有化；错误统一 `DomainError`、Outcome Result 化、实现体经 wire 工厂封装后 facade 死面清零（死 0、crate 内根折返 0），`facade.config.root-exports` 白名单与引擎真值一致。

## 四、error 重构（DomainError 机制）

### 错误三职责分离

| 职责 | 载体 | 消费方式 |
|---|---|---|
| 控制流（retry/fallback） | `category()` | `Storage` / `Invalid` / `Unavailable` |
| 呈现（模型/TUI） | `Display` | 生成点定型的中文文案 |
| 诊断 | `source_error()` + `domain` 标签 | 日志侧展开 |

### 规则

1. 细变体枚举全部 `pub(crate)`——变体只服务文案生成（`Display` 中文，变体间互异）；
2. 跨界唯一 `share::error::DomainError`，经 `From` 折叠层（分类映射 + 内部错误进 source 链）；
3. 聚合总线（`#[from]` 透传枚举，如 `MainSessionError`）逐步消亡——签名直传 `Result<_, DomainError>`；
4. 消费方零细变体 match——控制流走 `category()`，呈现走 `Display`，测试断言用 `message()` 前缀或 `category()` 判定。

## 五、配套判例（实践沉淀）

- **实现体不占导出类**：工厂返回 `Arc<dyn Trait>`，类型私有（policy `configured`/`allow_all`）；
- **注入优先闭包而非 trait**：`Fn() -> Mode` 表达"动态读一个值"足够（`PolicyModeReader` 消亡教训）；
- **多阶段协议不等于泄漏**：有真实跨 crate 阶段消费（如 GateAware 在 session-switch gate 锁内编排 Memory 预开启）的 prepare/commit 是协作协议，保留（config 三段判定）；
- **窄 trait + 组合句柄**是正形态：ISP 拆面 + 句柄聚合（`Workspace` 三 accessor + 隔离派生）；
- **注入式存储句柄归角色**：`NativeConfigStore`（`AuditStore` 同判）——wire 签名载荷，非数据非工厂。

## 六、执行 SOP（每 crate）

1. 全量导出清单 → 逐项过四问（§一）；
2. 命名映射四类语法（§二）+ 五类收录判据（§三），机械改名 + 消费方适配；
3. 错误折叠（§四）：内部化 → `From` → 签名换 → 消费方断言改形态；
4. 白名单与 construction_symbols 同步；
5. lib.rs PL 表格重写（DDD 五类分组 + 消费者 + 判定注释）；
6. 三重验证：workspace 测试全过 / clippy 0 / guard 90 rules 0 violations。
