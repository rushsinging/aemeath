# Memory · 端口与适配器

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Memory BC 的对外端口、NoOpMemory（Sub）、Storage 边界与 Composition Root 装配。**只描述目标态**；实现差距见 [迁移治理](../../03-engineering/migration-governance.md)。

## 1. MemoryPort

`MemoryPort` 是 Memory BC 拥有并发布的入站 façade / OHS，覆盖记忆的全部操作。Context Management、Runtime 的 Reflection 编排、MemoryTool 与 CommandRouter 按需消费；CLI / TUI **NEVER** 绕过 `AgentClient` 直接持有它。

```rust
#[async_trait]
trait MemoryPort: Send + Sync {
    // —— 检索 ——
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult;
    fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult;

    // —— 写入 ——
    async fn write(&self, entry: MemoryEntry) -> Result<WriteResult, MemoryError>;
    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError>;
    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError>;
    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError>;
    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError>;
    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError>;

    // —— 归档 / 淘汰 ——
    async fn archive(&self, ids: &[MemoryId]) -> Result<(), MemoryError>;
    async fn compact(&self) -> Result<CompactResult, MemoryError>;

    // —— 管理 / 查询 ——
    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry>;
    fn stats(&self) -> MemoryStats;
}

struct MemoryQuery {
    limit: usize,
    layer: Option<MemoryLayer>,
    category: Option<MemoryCategory>,
    now: u64,
}

struct MemorySearchQuery {
    text: String,
    limit: usize,
    layer: Option<MemoryLayer>,
    category: Option<MemoryCategory>,
    include_archive: bool,
    now: u64,
}

enum WriteResult {
    Added { id: MemoryId },
    Merged { existing_id: MemoryId },
    NeedsEviction { candidates: Vec<MemoryEntry> },
    NoOp,
}

enum MemoryError {
    InvalidEntry,
    Reflection(ReflectionError),
    Storage(MemoryStorageErrorKind),
}

struct MemoryStats {
    global_count: usize,
    global_archive_count: usize,
    project_count: usize,
    project_archive_count: usize,
}

enum MemoryStorageErrorKind {
    PermissionDenied,
    DiskFull,
    Serialization,
    ConcurrentWrite,
    CorruptTransaction,
    Io,
}
```

### 设计约束

- **MUST NOT** 返回内部 `MemoryStore` 实例或文件路径。
- **MUST NOT** 暴露文件 I/O 细节（路径、序列化格式）。
- **MUST NOT** 依赖 ProviderPort（Reflection 的 LLM 调用由 Runtime 编排）。
- **MUST** 所有可能落盘的 mutation 传播结构化 `MemoryError`；permission、disk-full、serialization 与 atomic-write 失败 **NEVER** 被压成 `false` / `()` 或假成功。Memory BC 把 Storage adapter 错误 ACL 为稳定 `MemoryStorageErrorKind`，**NEVER** 泄漏具体 adapter error。
- **MUST** Storage 返回 `CorruptTransaction` 时映射为同名稳定类别并 fail closed；其 quarantine receipt 只进入诊断 / 恢复 UI，Memory **NEVER** 把被隔离的 dataset 当作空 store。领域 JSON/schema 校验失败仍使用 `ActiveStoreCorrupt` / `ArchiveStoreCorrupt`，**NEVER** 与 Storage crash-protocol corruption 混为一类。
- **MUST** `open_for_project` 在返回前完成 dataset recovery、eager-read 与验证；`MemoryService` 此后持有唯一已验证的 in-memory active + archive state。`retrieve_for_inject` / `search` / `list` / `stats` 只读该 state，**NEVER** 做文件 I/O、lazy decode 或隐式 touch，因此可以返回非 `Result` 的纯查询值。
- **MUST** 所有 mutation 使用“复制 live state + expected dataset revision → 应用领域规则得到 candidate → CAS durable commit → 无失败 publish candidate + returned revision”协议。Storage 返回 `Err` 时保证尚未提交，live state 保持旧版本；返回 committed receipt 时无论是否带 recovery warning，都必须发布 candidate。**NEVER** 先修改 cache 再吞掉写失败，也 **NEVER** 把 committed warning 映射成普通 `MemoryError`。
- **MUST** `retrieve_for_inject` 不 touch（只读，避免排序漂移）；旧 mutating `top_for_inject` **NEVER** 出现在 Target API。
- **MUST** `retrieve_for_inject` 与 `search` 返回同一个 `MemorySearchResult` envelope：前者标记 `InjectionPriority` 且只含 active eligible hit，后者完整携带 retrieval mode、relevance、archive/outdated/TTL 状态。精确定义只见 [检索与注入](02-retrieval-and-injection.md)，本文件 **NEVER** 复制第二份类型。
- **MUST** `search` 按 query 的 `include_archive` 跨 active + archive 检索且不隐式修改 access_count；需要访问统计时必须另发显式、fallible mutation。
- **MUST** `compact` 跳过 pinned 条目；`archive` / `compact` 对每个受影响 layer 的 active + archive 成对提交，**NEVER** 暴露只移动一边的半归档。

## 2. ReflectionPromptPort

`ReflectionPromptPort` 把 Reflection 的领域逻辑暴露给 Runtime 编排。Memory BC 自身不调 LLM。

```rust
trait ReflectionPromptPort: Send + Sync {
    /// 构建反思 prompt（纯函数）
    fn build_prompt(
        &self,
        project_memory: &str,
        recent_summary: &str,
        lang: &str,
    ) -> String;

    /// 解析 LLM 返回的 JSON
    fn parse_output(&self, raw: &str) -> Result<ReflectionOutput, ReflectionError>;

    /// 格式化反思结果供 TUI 展示
    fn format_output(&self, output: &ReflectionOutput, lang: &str) -> String;

    /// 纯格式化当前 Run 经 MemoryPort 取得的项目记忆
    fn format_memory_summary(&self, entries: &[MemoryEntry]) -> String;

    /// 从消息列表构建对话摘要
    fn recent_messages_summary(&self, messages: &[Message], max_chars: usize) -> String;
}
```

### 为什么不合并到 MemoryPort

1. **职责分离**：MemoryPort 管记忆 CRUD、检索与对当前实例应用 Reflection；ReflectionPromptPort 只做纯 prompt / parse / format。Runtime 必须把当前 Run 的同一 `MemoryPort` Arc 用于检索和 `apply_reflection`，纯 Reflection port **NEVER** 隐式选择 store。
2. **Sub 隔离**：Sub Run 装配 `NoOpMemory`（MemoryPort 的空实现），但 Reflection 在 Sub 中完全不触发——不需要 NoOpReflection。
3. **演进独立**：检索升级（BM25/embedding）和 Reflection prompt 优化可以独立演进。

## 3. NoOpMemory（Sub Run）

Sub Run 装配 `NoOpMemory`——所有方法返回空值/空集合，不读写不报错：

```rust
struct NoOpMemory;

#[async_trait]
impl MemoryPort for NoOpMemory {
    fn retrieve_for_inject(&self, _: &MemoryQuery) -> MemorySearchResult {
        MemorySearchResult::empty(MemoryRetrievalMode::Disabled)
    }
    fn search(&self, _: &MemorySearchQuery) -> MemorySearchResult {
        MemorySearchResult::empty(MemoryRetrievalMode::Disabled)
    }
    async fn write(&self, _: MemoryEntry) -> Result<WriteResult, MemoryError> { Ok(WriteResult::NoOp) }
    async fn update(&self, _: &MemoryId, _: &str) -> Result<bool, MemoryError> { Ok(false) }
    async fn delete(&self, _: &MemoryId) -> Result<bool, MemoryError> { Ok(false) }
    async fn pin(&self, _: &MemoryId, _: bool) -> Result<bool, MemoryError> { Ok(false) }
    async fn mark_outdated(&self, _: &MemoryId) -> Result<bool, MemoryError> { Ok(false) }
    async fn apply_reflection(&self, _: &ReflectionOutput) -> Result<ReflectionApplyResult, MemoryError> { Ok(ReflectionApplyResult::NoOp) }
    async fn archive(&self, _: &[MemoryId]) -> Result<(), MemoryError> { Ok(()) }
    async fn compact(&self) -> Result<CompactResult, MemoryError> { Ok(CompactResult { archived: 0, remaining: 0 }) }
    fn list(&self, _: Option<MemoryLayer>) -> Vec<MemoryEntry> { Vec::new() }
    fn stats(&self) -> MemoryStats { MemoryStats { global_count: 0, global_archive_count: 0, project_count: 0, project_archive_count: 0 } }
}
```

- Sub 不读记忆（查询返回 `mode=Disabled` 的空 envelope，**NEVER** 冒充 BM25 / InjectionPriority 的普通空命中）。
- Sub 不写记忆（mutation 返回显式 `NoOp` / `false` / 空结果，不伪报 `Added`）。
- Sub 不触发 Reflection（Runtime 根据 `MemoryMode::Disabled` 跳过）。
- Main 可通过 `share_memory` 参数显式给 Sub 开启注入；此时 **MUST** clone 父 Run 在 shared lease 下持有的同一 MemoryPort Arc，**NEVER** 为同一 ProjectIdentity 再打开第二个 service。

## 4. Storage 边界

### 职责拆分

| 层 | 归属 | 职责 |
|---|---|---|
| 领域模型 | Memory BC 私有 model capability | MemoryEntry、枚举、scoring、dedup、format——纯数据 + 纯函数 |
| 领域服务 | Memory BC 的 MemoryService | MemoryPort 实现：检索、去重判定、淘汰候选、apply |
| 文件 I/O | Storage adapter | read_entries / write_entries / 路径解析 / 原子写 / 损坏兜底 |

```text
Memory BC                        Storage BC
┌─────────────────┐              ┌──────────────────┐
│ MemoryPort impl │ ──读写委托──▶ │ MemoryStorageAdapter │
│ (领域逻辑)       │              │ (文件 I/O)        │
│ - scoring       │              │ - read_entries    │
│ - dedup         │              │ - write_entries   │
│ - retrieval     │              │ - path resolve    │
│ - apply         │              │ - atomic write    │
└─────────────────┘              └──────────────────┘
```

MemoryPort 实现持有 `MemoryStorageAdapter`（或等价窄 Storage port），领域逻辑在 Memory 侧，I/O 委托给 Storage 侧。Memory 代码 **NEVER** import Storage 的具体 store 类型；Storage adapter **NEVER** 实现 scoring、dedup、检索排序或 Reflection apply 规则。

`MemoryService` **MUST** 使用独立的 async mutation mutex 串行化本实例的 candidate / durable / publish 用例，并以短时同步 `RwLock<CommittedMemoryState { dataset, revision }>` 服务查询；它 **NEVER** 在 storage await 期间持有 state write lock。mutation 从同一已提交 state clone candidate 与 expected `DatasetRevision`，随后把 candidate bytes 与 expected revision 交给基于 Storage [`AtomicDatasetPort`](../storage/README.md#42-多-member-dataset-事务) 的 `MemoryDatasetStore`。内部 owned task 持有 mutation permit 完成 CAS commit 与无失败 state publish；caller future 在 handoff 前可取消，handoff 后丢弃只停止等待，**NEVER** 取消已开始的 durable 事务。

跨进程锁只让提交依次执行，**NEVER** 单独防止 stale writer。`MemoryDatasetStore` **MUST** 原样回传 open 时 `read_consistent` 得到的 revision，并在 `commit_atomic(expected, members, ...)` 的 CAS 冲突时映射为 `MemoryStorageErrorKind::ConcurrentWrite`。第一次冲突时，MemoryService 在 mutation mutex 内重新 `read_consistent`、验证并发布外部已提交的完整 state / revision，再基于新 state **重新执行领域命令一次**；若再次冲突则返回结构化错误，绝不覆盖。普通查询仍只读内存：v0.1.0 不提供跨进程实时 watch，其他进程的提交在下一次 open 或本实例冲突刷新后可见。

`DatasetCommitReceipt::Visible` 与 `RecoveryPending` 都代表 committed。MemoryService 对两者都无失败发布 candidate 与 receipt revision；warning 只进入诊断。只有 Storage 明确保证 `Err = NotCommitted` 时，Memory mutation 才返回错误并保留旧 state。

普通 write / update / delete 也遵循同一顺序；`archive` / `compact` **MUST** 把每个受影响 layer 的 active + archive member 放进同一 dataset transaction。若一次命令跨 Global / Project layer，全部受影响 member 必须进入同一 batch，或在领域 API 明确拆成两个可观察命令；**NEVER** 在一个成功 / 失败结果下静默部分提交。Storage 的 dataset lock、journal 与 recovery 是唯一 crash protocol，Memory adapter **NEVER** 复制一套。

## 5. Composition Root 装配

```rust
#[async_trait]
trait ProjectMemoryOpener: Send + Sync {
    async fn open_for_project(
        &self,
        identity: &ProjectIdentity,
        config: &MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenError>;
}

enum MemoryOpenError {
    PermissionDenied,
    StorageTransactionCorrupt,
    ActiveStoreCorrupt,
    ArchiveStoreCorrupt,
    UnsupportedSchema { version: u32 },
    LegacyKeyConflict,
    LegacyMigrationFailed,
    Io,
}

fn assemble_reflection(config: &ConfigSnapshot) -> Arc<dyn ReflectionPromptPort> {
    Arc::new(ReflectionEngine::new(config.reflection_config()))
}
```

- **Main agent 打开**：Composition 先准备 project-aware Config，再按 `WorkspaceRead::project_identity()` 与 candidate `MemoryConfig` await 一次 `open_for_project`，把真实 `MemoryService` 交给 Context-owned active Session slot；每个 Main Run 在 shared lease 下取得同一 Arc 并同时注入 Context、Runtime、MemoryTool 与 Reflection apply。
- **Sub Run（Disabled）**：装配 `NoOpMemory`；Reflection 不触发（Runtime 按 `MemoryMode::Disabled` 跳过）。
- **Sub Run（Enabled，Main 显式 share）**：clone 父 Run 当前 Arc；父 shared lease 覆盖 Sub 生命周期。
- **运行期 resume**：exclusive session-switch lease 下先 prepare Project，再 prepare target Config，然后 await `open_for_project(target_identity, prepared_config.memory_config())`；只有它与 Task prepare 都成功后才安装 candidate Arc。

### 装配约束

- **MUST** MemoryPort 实例由 Composition Root 构造，不在核心或适配器内私自 `new`。
- **MUST** MemoryStorageAdapter 由 Composition Root 注入 MemoryService。
- **MUST NOT** MemoryService 直接 `std::fs::read` / `std::fs::write`——经 Storage adapter。
- **MUST** `open_for_project` 应用传入的 `MemoryConfig`，eager-read 并验证 active + archive 文件、schema 与权限后才返回可用 Arc；它 **NEVER** 自行读取全局 current ConfigSnapshot，lazy open **NEVER** 把 fallible I/O 推迟到 resume commit 之后。
- **MUST** open 先在 dataset lock 下完成任何 prepared journal 的 roll-forward / rollback，再读取同一 committed generation；任一 recovery / decode / invariant 失败返回 `MemoryOpenError`，**NEVER** 发布部分 service。

## 6. 持久化格式

### 文件布局

```text
~/.agents/memory/
├── _global.json              # Global 层 active 条目
├── _global_archive.json      # Global 层归档条目
├── {project_file_name}.json       # Project 层 active 条目
└── {project_file_name}_archive.json  # Project 层归档条目
```

### 序列化格式

- JSON 数组：`Vec<MemoryEntry>` 序列化为 `[...]`。
- 枚举使用 `snake_case`：`"global"` / `"project"` / `"fact"` / `"decision"` / `"llm"` 等。
- 可选字段使用 `#[serde(default, skip_serializing_if = "Option::is_none")]`。
- `tags` 使用 `#[serde(default)]`。

### project_file_name

`project_file_name(&ProjectIdentity)` **MUST** 对完整 identity（canonical `initial_cwd` + optional canonical `git_common_dir`）做稳定、域分隔的 hash，并生成带 schema 前缀的 `v2_<hash>` 安全文件名；只取目录 basename **NEVER** 足以区分不同 canonical project 路径。路径编码逻辑归 Storage adapter，原始绝对路径 **NEVER** 直接成为文件名。当前 identity 是路径身份，不含 repository object id；同一路径原地 `git init` / 替换仓库仍会得到同一 key，若未来必须区分该场景，**MUST** 先扩展 Project-owned identity PL 并版本化为新 key，**NEVER** 在 Memory 私自 probe Git。

### legacy project key 迁移

完整 identity key 上线时 **MUST** 保留显式兼容 reader，避免旧 cwd-derived memory 文件被误判为“空项目”：

1. 每次 open 先**无副作用 existence-probe** new active、new archive、legacy active、legacy archive 与 migration journal；只有分类完成后才读取数据。active / archive 的单侧缺失在无 journal 时表示该侧为空，是合法 dataset，**NEVER** 单凭缺一个文件判断半迁移。
2. journal 存在时优先按其记录的 staged / published phase resume 或 rollback；在 journal 结案前 **NEVER** 走普通选择逻辑。
3. 任一 new 文件存在且任一 legacy 文件也存在、又没有证明同一迁移来源的 journal 时，返回 `LegacyKeyConflict` 并保留两边，**NEVER** 静默 merge、覆盖或任选其一。只有两个 new 文件都不存在且至少一个 legacy 文件存在时才启动 legacy migration；两边都不存在则打开规范空 store。
4. legacy active / archive **MUST** 先完整读取（缺失侧按空集合）并验证 schema、权限与 entry 不变量，任一损坏返回结构化 `MemoryOpenError`，**NEVER** 以空 store 覆盖。
5. 迁移 **MUST** 复用 Storage `AtomicDatasetPort` 的 dataset lock、expected revision、stage、journal / commit marker 与 recovery primitive：先以 `read_consistent` 取得 new-key dataset revision，再把两份 new-key member作为一笔 CAS commit；进程中断后由 `read_consistent` 在开放 service 前 roll-forward / rollback，旧文件在 committed 证据完成前保持不动。Memory 只提供 legacy → candidate 的领域转换，**NEVER** 自建第二套事务算法。
6. 成功后 active service 与所有后续 writer **MUST** 只写 versioned new key；legacy 文件可在独立退役步骤备份 / 删除，并记录来源诊断。

## 7. 机械边界验收

Target 要求机械守卫证明：production Memory wiring 只由 Composition Root 发起；业务调用方只接收 `MemoryPort` / `ReflectionPromptPort`，不能直接构造或获得 `MemoryService` / Storage adapter；Memory 不能直接使用文件 I/O。具体守卫脚本、启用状态、临时白名单与替换责任只见 [Architecture Guards](../../03-engineering/architecture-guards.md) 和 [Migration Governance](../../03-engineering/migration-governance.md)，本文 **NEVER** 声称尚未登记的规则已在 CI / Stop 生效。

## 8. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型：[01-domain-model.md](01-domain-model.md)
- 检索与注入：[02-retrieval-and-injection.md](02-retrieval-and-injection.md)
- Reflection 引擎：[03-reflection.md](03-reflection.md)
- Runtime 端口（MemoryPort 消费方）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory 集成关系）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 依赖规则：[../../01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：MemoryPort trait、ReflectionPromptPort、NoOpMemory、Storage 边界、Composition Root、现状缺口 M1-M10 | #789 |
| 2026-07-14 | 将构造守卫语言对齐 capability-first 组织，移除固定横向层命名 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 增加 DatasetRevision CAS、committed receipt 发布语义与跨实例冲突刷新，移除 Current 路径和未登记守卫声明 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 查询统一返回带 retrieval mode、relevance 与 archive/outdated/TTL 状态的 MemorySearchResult envelope | [#972](https://github.com/rushsinging/aemeath/issues/972) |
