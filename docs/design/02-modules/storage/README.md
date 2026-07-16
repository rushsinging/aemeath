# Storage（通用域）

> 层级：02-modules / storage（模块摘要设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#793（S2）
> Storage 提供可靠的物理持久化机制，但不拥有 Session、Memory、Task、Workspace、History、Tool Result 或 Audit Event 的业务语义。

## 1. 模块定位

Storage 是数据 BC 与物理介质之间的机制边界：

```text
      Context Management / Memory / Audit / Tool
                     │ Snapshot / Blob + StorageKey
                     ▼
               Storage Port
        ┌────────────┼─────────────┐
        ▼            ▼             ▼
   Atomic File    Safe Reader   Other Backend
    Adapter       + Recovery      Adapter
```

数据 BC 决定“保存什么、何时保存、保留多久、如何迁移”；Storage 决定“如何安全写入、如何读取、如何隔离损坏、如何约束物理路径”。持久化状态不等于拥有领域状态。

## 2. 核心决策

1. **数据本体归原 BC**：Session、Memory Entry、Task Snapshot、Workspace Snapshot、Tool Result 与 Audit Event 的 schema、不变量、迁移和生命周期由各自 BC 拥有。
2. **Storage 发布机制语言**：跨边界只交换 `StorageKey`、字节/序列化 payload、读写选项、结果和结构化错误；Storage 不 import 领域聚合。
3. **端口分两层归属**：Storage 只拥有机械 `AtomicBlobPort` 与 `AtomicDatasetPort` 两个整值原子替换 OHS，**NEVER** 发布 append-log OHS——stage/fsync/rename 的整值替换协议无法安全拼出增量 append + 逐行 flush 语义（每次追加都要整值重写，durability 与并发追加顺序都不成立）；数据 BC 拥有 `SessionSnapshotStore`、`UsageAppendStorePort` 等窄出站端口，其 integration adapter 位于消费方外层或 Composition，不让 Storage 反向依赖领域模型。`UsageAppendStorePort` 由 Audit adapter 直接以 file append detail 实现，只复用 Storage 发布的路径安全 primitive（`SafePathSegment` 校验、受约束根目录句柄解析），不经过 `AtomicBlobPort`/`AtomicDatasetPort`。
4. **原子可见**：单 blob 替换后，读者只能看见完整旧值或完整新值；由多个 member 构成的逻辑 dataset 必须经 dataset-level lock + journal / commit marker 作为一笔可恢复事务提交，领域 reader **NEVER** 看见成员混代。
5. **保留上一代物理完整值**：启用恢复代际的 namespace 在替换已有值时保留上一代完整 bytes；是否符合领域 schema 只能由数据 BC 验证。
6. **损坏不静默丢弃**：数据 BC 验证主值失败后可机械读取上一代，并显式请求 promote 或 quarantine；不得自动当作空数据继续。
7. **路径安全覆盖竞态**：除 segment 词法校验外，文件 adapter 必须使用受约束目录句柄、no-follow/create-new 或等价机制，防止 symlink 与 TOCTOU 越出根目录。
8. **业务策略不下沉**：阈值、preview、retention、级联删除、schema migration、compact/eviction 均留在拥有数据语义的 BC；Config 只提供静态默认值。
9. **无 Run checkpoint**：Storage 不建设 durable Run、Model Invocation checkpoint 或未完成 ToolCall 自动重放。
10. **多 writer 不丢更新**：dataset read 返回 opaque revision；commit 在同一跨进程锁内比较 expected revision，只有匹配时才进入 prepared。锁只提供串行执行，revision CAS 才证明两个先后获得锁的 writer 不会用陈旧快照覆盖新 generation。
11. **事务损坏是独立 typed failure**：journal / primary / member digest 无法机械归入完整旧代或新代时返回 `StorageErrorKind::CorruptTransaction`，并携带 quarantine disposition；**NEVER** 降级为普通 `Io`、`NotFound` 或空 dataset。

## 3. Target 物理目录

Storage 采用 Hexagonal + Clean 组织（`domain + ports + adapters`）。三个拥有独立协议与故障测试边界的机制——`safe_path`（路径词法校验）、`atomic_blob`（整值 blob 原子替换协议）、`atomic_dataset`（多 member dataset 事务协议）——作为同一 Storage BC 的 domain 模块共置；Storage-owned OHS 集中在 `ports/`，文件系统技术 detail 终止在 `adapters/`。

选择该结构不仅为了语义分层，也为了**易守卫、防漂移、防劣化**：固定的层名与 `domain ← ports ← adapters` 方向可由静态 Guard 直接证明；Guard **MUST** 阻止 `domain/` 使用 `std::fs`/物理 `PathBuf` 或 import `adapters`，阻止 `ports/` 依赖具体 adapter，阻止 adapter 类型进入 crate-root Published Language，并限制 `lib.rs` 只发布稳定 PL/OHS。Storage 不叠加 `capabilities/`，因为它会形成第二套物理边界并提高依赖矩阵守卫成本；三个机制通过 domain 子模块表达，仍共享同一 Hexagonal 约束：

```text
src/
├── lib.rs                       # 窄 façade：发布 AtomicBlobPort / AtomicDatasetPort OHS，composition-only wiring
├── domain.rs                    # 领域策略入口
├── domain/
│   ├── safe_path.rs             #   SafePathSegment 词法校验、受约束根目录句柄解析
│   ├── atomic_blob.rs           #   AtomicBlobPort 用例策略：write_atomic / read / promote_previous / quarantine
│   ├── atomic_dataset.rs        #   AtomicDatasetPort 用例策略：read_manifest / read_consistent / commit_atomic
│   └── published_language.rs    #   StorageKey / DatasetKey / Quarantine PL
├── ports.rs                     # 对外端口定义
│   ├── atomic_blob_port.rs      #   AtomicBlobPort OHS
│   └── atomic_dataset_port.rs   #   AtomicDatasetPort OHS
└── adapters/
    ├── blob_filesystem.rs       #   blob stage/fsync/rename/journal 文件系统实现
    └── dataset_filesystem.rs    #   dataset lock / journal 文件系统实现
```

- `lib.rs` 只受控 re-export `AtomicBlobPort` / `AtomicDatasetPort` 与 §4 Published Language 类型，**NEVER** 转发内部结构。
- `ports/` 只定义 Storage-owned `AtomicBlobPort` / `AtomicDatasetPort` OHS，并依赖 `domain` Published Language；它 **NEVER** 成为所有 trait 的垃圾桶，也不容纳消费方的 Session/Memory/Audit 出站端口。
- `adapters/` 内的文件系统实现是各用例的私有技术 detail；`atomic_blob` 与 `atomic_dataset` 各自拥有自己的 stage/fsync/rename/journal 实现，互不复用同一文件系统 adapter；这正是 §3.5 所述"Storage 私有 backend SPI"的物理落点——driver 只在 `adapters/` 内实现该私有 SPI，对外仍只发布 `AtomicBlobPort` / `AtomicDatasetPort`。
- `safe_path` 是被 `atomic_blob` / `atomic_dataset` 消费的独立 domain 子模块：它拥有自己的校验协议与测试夹具，**NEVER** 因"看似工具函数"被内联进另外两个模块。
- Storage **NEVER** 建立按存储技术命名的模块级横向技术目录；sled 等未来 backend 若引入，仍 **MUST** 落在 `adapters/` 内，不形成横向替换层。

## 4. Published Language

以下签名表达语义，不锁定具体 Rust API：

```rust
struct StorageKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

/// SafePathSegment 是受约束的路径段（词法校验：禁止 `..` / `/` / `\` / 前导 `.`）。
struct SafePathSegment(String);

enum StorageNamespace {
    Session,
    Memory,
    Task,
    History,
    ToolResult,
    AuditUsage,
    Config,
    Workspace,
    Cost,
}

struct WriteOptions {
    durability: Durability,
}

enum Durability { ProcessCrashSafe, BestEffort }
enum Generation { Primary, Previous }

enum PreviousPolicy { Retain, Discard }

impl StorageNamespace {
    /// namespace-owned 静态策略；调用方不能逐次关闭。
    fn previous_policy(self) -> PreviousPolicy;
}

struct DeleteOptions {
    include_quarantine: bool,
}

impl Default for DeleteOptions {
    // 业务删除默认彻底删除隐藏副本；取证保留必须显式 opt out
    fn default() -> Self { Self { include_quarantine: true } }
}

struct BlobRead {
    generation: Generation,
    bytes: Vec<u8>,
}

/// read() 只返回调用方在 `generation` 参数中显式请求的那一代机械字节；
/// 请求的 generation 不存在时返回 NotFound，NEVER 在 read() 内部自动跨代 fallback。
/// 领域需要检查 Previous 时，必须显式再次调用 read(key, Generation::Previous)。
enum ReadOutcome {
    Found(BlobRead),
    NotFound,
}

/// delete_all_generations() 的结果。
struct DeleteOutcome {
    deleted_primary: bool,
    deleted_previous: bool,
    deleted_quarantine: bool,
}

/// list_primary() 的单个条目。
struct StorageEntry {
    key: StorageKey,
    size_bytes: usize,
    generation: Generation,
}

struct DatasetKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

struct DatasetMember {
    name: SafePathSegment,
    bytes: Vec<u8>,
}

/// Storage 生成的完整 dataset 内容指纹；字段私有，消费方只比较或原样回传。
struct DatasetRevision(/* domain-separated digest */);

struct DatasetRead {
    revision: DatasetRevision,
    members: Vec<DatasetMember>,
}

/// Storage 在提交时冻结的权威完整成员名集合；调用方不需要预先猜测成员名即可发现当前 generation 有哪些 member。
struct DatasetManifest {
    revision: DatasetRevision,
    members: Vec<SafePathSegment>,
}

/// 返回该值即证明 primary 已逻辑提交；任何后提交点问题只能进入 warning。
struct WriteReceipt {
    warning: Option<CommitWarning>,
}

struct DatasetCommitReceipt {
    revision: DatasetRevision,
    visibility: DatasetCommitVisibility,
    warning: Option<CommitWarning>,
}

enum DatasetCommitVisibility {
    Visible,
    RecoveryPending,
}

enum CommitWarning {
    PreviousPromotionPending,
    JournalCleanupPending,
    MemberPublishRecoveryPending,
}

/// promote_previous() 的幂等结果。
enum PromoteOutcome {
    Promoted(WriteReceipt),
    AlreadyPromoted,
    NotFound,
}

/// quarantine() 只移动显式指定的代；缺失时不创建虚假 artifact。
enum QuarantineOutcome {
    Moved(QuarantineReceipt),
    AlreadyAbsent,
}

struct QuarantineId(SafePathSegment);

struct QuarantinedArtifact {
    scope: TransactionScope,
    generation: Generation,
    bytes: Vec<u8>,
    reason: QuarantineReason,
}

enum QuarantineReason {
    DigestMismatch,
    DecoderRejected,
    PromoteFromCorrupt,
}

struct QuarantineReceipt {
    id: QuarantineId,
    artifacts: Vec<QuarantinedArtifact>,
}

enum TransactionScope { Blob, Dataset }

enum CorruptTransactionReason {
    InvalidJournal,
    PrimaryDigestMatchesNeitherGeneration,
    CommittedDigestMismatch,
    OrphanPreviousDigestMismatch,
    DatasetMemberDigestMismatch,
}

enum QuarantineDisposition {
    Completed(QuarantineReceipt),
    Failed(QuarantineFailureKind),
}

enum QuarantineFailureKind { Io, PermissionDenied }

struct CorruptTransactionError {
    scope: TransactionScope,
    reason: CorruptTransactionReason,
    quarantine: QuarantineDisposition,
}

// ReadOutcome 统一定义在 §4 Published Language 前文（见 enum ReadOutcome 定义），此处不再重复。
// StorageError 统一为 StorageErrorKind（见下）。

enum StorageErrorKind {
    InvalidKey,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    ConcurrentWrite,
    CorruptTransaction(CorruptTransactionError),
}
```

> **Audit UsageAppendStorePort 所有权**：`UsageAppendStorePort` 是 **Audit BC 拥有的出站端口**，不是 Storage 发布的通用端口。`AtomicBlobPort`/`AtomicDatasetPort` 的原子性建立在“整值 stage → fsync → rename”协议上，天然拼不出增量 append + 逐行 flush 语义；因此 Audit adapter **MUST** 直接以 file append（open-append 等价物 + write + fsync）detail 实现 append-log，只复用 Storage 发布的路径安全 primitive（`SafePathSegment` 校验、受约束根目录句柄解析），**NEVER** 组合调用 `AtomicBlobPort`/`AtomicDatasetPort` 来模拟追加。端口 trait 的定义、调用和语义归属 **MUST** 属于 Audit；Storage 只提供原子读写、路径安全和损坏兜底**机制**，不发布 append-log OHS。这样保持 Storage 的 blob/dataset 级抽象不被 append 语义污染，也避免两个 BC 同时声称拥有同一端口。

`StorageKey` 表达逻辑位置，不暴露用户主目录或绝对路径。物理路径由 adapter 根据 ConfigSnapshot 提供的根目录与 namespace policy 解析。namespace policy 固定是否保留上一代；调用方不能逐次关闭该安全属性。Session、Memory、Task、History、ToolResult、Config、Workspace 与 Cost 使用 `PreviousPolicy::Retain`；AuditUsage 的增量 append 不经 AtomicBlobPort，因此使用 `Discard`。未来新增 namespace **MUST** 显式选择策略，禁止依赖默认分支。

`promote_previous` 的幂等语义由 typed outcome 冻结：Previous 存在时返回 `Promoted(receipt)`；Previous 已在同一 adapter 生命周期内被该 key 的上一次 promote 成功消费且 Primary 仍存在时返回 `AlreadyPromoted`；从未存在可 promote generation 或 Primary/Previous 均不存在时返回 `NotFound`。实现 **NEVER** 只凭“Previous 缺失 + Primary 存在”猜测 `AlreadyPromoted`，必须有 Storage-owned 提交证据；该跨 reopen 证据与 crash-safe 幂等由 #882 journal 承接，#881 仅保证同一进程实例内的机械幂等。`quarantine` 缺失指定 generation 时返回 `AlreadyAbsent`，不创建 artifact id、不移动另一代；移动成功 receipt 必须原样记录 generation、scope 与 reason。

`CorruptTransaction` 只表示 **Storage 自己的 crash protocol 证据互相矛盾**；领域 decoder 拒绝一份机械完整的 JSON / bytes 仍由所属数据 BC 处理，并通过普通 `quarantine()` 命令隔离，**NEVER** 伪装成 Storage transaction corruption。错误不暴露绝对路径、nonce 或原始内容；quarantine 失败也 **MUST** 在 `QuarantineDisposition::Failed` 中保留原始 corruption reason。

### 4.1 端口形态

```rust
trait AtomicBlobPort: Send + Sync {
    async fn read(
        &self,
        key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError>;
    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError>;
    async fn promote_previous(&self, key: &StorageKey) -> Result<PromoteOutcome, StorageError>;
    /// 显式隔离某一 generation / transaction scope；reason 由调用方选择稳定 typed 类别。
    async fn quarantine(
        &self,
        key: &StorageKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError>;
    async fn delete_all_generations(
        &self,
        key: &StorageKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError>;
    async fn list_primary(&self, prefix: &StorageKey) -> Result<Vec<StorageEntry>, StorageError>;
}

trait AtomicDatasetPort: Send + Sync {
    /// 先恢复未结事务，再返回当前 generation 的 Storage-owned 完整成员清单。
    async fn read_manifest(&self, dataset: &DatasetKey) -> Result<DatasetManifest, StorageError>;

    /// 先恢复未结事务，再在同一 dataset lock 下读取请求的完整 member 集。只服务当前（primary）generation。
    async fn read_consistent(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetRead, StorageError>;

    /// 读取 previous generation 的完整成员集；缺失返回 NotFound。NEVER 由 read_consistent 自动降级到这里。
    async fn read_previous(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetRead, StorageError>;

    /// 仅当当前 revision 等于 expected 时提交全部 member；members 是新 generation 的完整清单，
    /// 当前 generation 中存在但未出现在 members 里的成员名视为显式 omitted=delete。
    /// Ok 永远表示新 generation 已逻辑 committed；Err 永远表示未提交。
    async fn commit_atomic(
        &self,
        dataset: &DatasetKey,
        expected: &DatasetRevision,
        members: &[DatasetMember],
        options: WriteOptions,
    ) -> Result<DatasetCommitReceipt, StorageError>;

    /// 跨越 previous → primary 互换这一提交点；成功后 previous 成为新的当前 generation。
    async fn promote_previous(&self, dataset: &DatasetKey) -> Result<DatasetCommitReceipt, StorageError>;

    /// 隔离指定 generation 的 dataset transaction / member 证据；领域 decoder reason 与 Storage crash reason 分离。
    async fn quarantine(
        &self,
        dataset: &DatasetKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineReceipt, StorageError>;
}
```

`DatasetManifest` 是 Storage 在提交时冻结的权威完整成员名集合：调用方不需要预先知道全部成员名即可发现当前 generation 有哪些 member，也不能自行拼凑成员清单绕过 Storage 记录的边界。`commit_atomic` 的 `members` 参数永远是新 generation 的完整替换清单，不是增量 patch：未出现在 `members` 中但存在于当前 generation 的成员名，Storage **MUST** 在同一 dataset lock/journal 内一并物理删除，不留孤儿文件；调用方要保留某个成员必须把它连同 bytes 一起放进 `members`。`read_consistent`/`read_manifest` 只返回当前 generation，**NEVER** 因为部分成员缺失就自动降级读取 previous；需要上一代时必须显式调用 `read_previous`。dataset 的 `promote_previous` 与 blob port 同名操作共享 committed+warning 语义；blob / dataset `quarantine` 必须由调用方显式传入 `generation + TransactionScope + QuarantineReason`，receipt 原样记录这些 typed facts，adapter **NEVER** 猜测领域 decoder reason。

`DatasetRevision` **MUST** 对规范排序后的 member name、存在性与 bytes 做域分隔摘要；空 dataset 也有稳定 revision。`read_consistent` 在恢复未结事务后返回它；消费方 **NEVER** 构造 revision。`commit_atomic` 在同一 dataset lock 内重新计算 committed revision，若与 `expected` 不同，必须在写 stage / prepared journal 前返回 `ConcurrentWrite`。因此跨进程锁负责串行，expected-revision CAS 负责防止 stale writer 覆盖。

数据 BC 定义更窄端口，例如 `SessionSnapshotStore`、`MemoryDatasetStore`、`AuditEventStore`。这些端口的 integration adapter 依赖数据 BC Snapshot PL，并在内部调用 Storage 的 `AtomicBlobPort` 或 `AtomicDatasetPort`；它位于消费方 adapter 层或 Composition，不进入 Storage BC。两个 Storage port 都只认识 key / member / bytes，**NEVER** 携带 `Session`、`Task` 或 `MemoryEntry` 类型；同一多文件不变量 **MUST** 复用 `AtomicDatasetPort`，不得由各领域 adapter 重写一套 crash protocol。

## 5. 原子写协议

目标文件 adapter 对启用上一代恢复的 namespace 采用可验证协议：

```text
1. 通过受约束根目录句柄解析 key；拒绝 symlink/no-follow 违规
2. 获取同 key 的进程内 + 跨进程写锁；先恢复或隔离旧 journal
3. create-new 随机 stage；write_all + file fsync，计算 new_digest
4. 若 primary 存在：生成 previous.next、fsync，并记录 old_digest；首次写记录 old=Absent
5. 原子写并 fsync prepared journal { nonce, old_digest|Absent, new_digest, phase=Prepared }，再 fsync 父目录
6. 原子 replace stage → primary；fsync 父目录（逻辑提交点）
7. 原子把 journal 更新为 phase=Committed 并 fsync；随后 promote previous.next → previous，再 fsync 父目录
8. 清理本事务临时文件 / journal，释放锁，返回 WriteReceipt
```

提交点在第 6 步：此前 crash 保留旧 primary；此后 crash 保留新 primary。rename 与 journal phase 更新无法原子完成，因此恢复 **NEVER** 只看 marker：在同一 key lock 下读取 journal 并校验 primary digest。`phase=Prepared` 且 primary digest 等于 `new_digest` 表示 rename 已提交，**MUST** 补写 Committed 并完成 previous promotion；等于 `old_digest`（或首次写仍 Absent）表示尚未提交，可清理 stage / previous.next / journal；与两者都不符时 **MUST** 隔离 journal、stage、primary 与 previous.next 的可识别证据，并返回 `StorageErrorKind::CorruptTransaction(CorruptTransactionError { reason: PrimaryDigestMatchesNeitherGeneration, ... })`，**NEVER** 猜测或删除上一代。`phase=Committed` **MUST** 验证 primary 等于 `new_digest` 后完成 promotion；不相等时返回 `CommittedDigestMismatch`。没有 journal 时可以清理不被引用的随机 stage；残留 `previous.next` 只有在其 digest 等于当前 primary（证明 crash 发生在 journal 前）时才可清理，否则返回 `OrphanPreviousDigestMismatch` 并 quarantine，**NEVER** 当作普通垃圾删除。

`write_atomic` 的任何 `Err` **MUST** 表示提交点尚未跨越；第 6 步之后若任一后续 I/O 报错，实现必须在锁内按同一 digest 判定恢复并返回带 `CommitWarning` 的 committed `WriteReceipt`，**NEVER** 返回普通 Err。调用方看到 receipt 必须发布新 live state，**NEVER** 把 warning 当作“仍是旧值”。首次写入没有 previous。所有 adapter 共享这张 crash-state 恢复表，不得自行选择清扫语义。

`ProcessCrashSafe` 表示 stage 文件和提交目录项都完成所需同步；`BestEffort` 只保证进程内原子可见。namespace 规定最低 durability，逐次 WriteOptions 只能提高，不能降低；平台无法兑现时返回 `UnsupportedDurability`。

### 5.1 关键不变量

- stage、primary 与 previous 位于同一文件系统；
- 临时名称不可预测且使用 create-new；
- 新 payload 同步完成前不得修改 primary；
- primary 在任何 crash point 都是完整旧值或完整新值；
- previous 只保存曾提交过的完整 primary bytes，但不承诺领域可解析；
- 同一 key 的进程内和跨进程写必须串行化；
- 成功或带 warning 返回时必须明确 committed/durability/previous 状态；
- 残留 stage/previous.next 不参与普通读取；启动恢复必须按 journal phase + old/new digest 完成、回滚或隔离事务；
- 所有 open/rename/delete 均相对受约束目录句柄执行，禁止跟随 symlink 越界。

### 5.2 多 member dataset 事务

`AtomicDatasetPort` 用于 active + archive、index + payload 等必须共同换代的逻辑数据集：

```text
1. 按 DatasetKey 获取进程内 + 跨进程排他锁；所有 dataset read 也先经该锁
2. 恢复或结案既有 journal，计算 committed DatasetRevision；与 expected 不同则返回 ConcurrentWrite，且未提交
3. 为全部 member create-new stage，write_all + fsync；校验 member 名称唯一并计算 new revision
4. 在覆盖任何 current member **之前**，从 manifest 指定的完整旧集合为每个 member 创建 `previous.next`（hard-link/copy-on-write/同文件系统 copy 均可，但必须 fsync）并写入 old manifest digest；任一失败仍处于 prepared 前，可清理并保留完整旧 generation
5. 写入含 transaction id、old/new member digest、expected/new revision、完整新 manifest（含待物理删除的 omitted 成员）与 phase=prepared 的 journal，并 fsync（逻辑提交点）
6. 逐 member 发布 staged generation（含物理删除 omitted 成员）；每步更新 journal phase并目录 fsync
7. 校验全部 new member 后写 committed marker；原子 promote 已完整 fsync 的 `previous.next` 集合及 old manifest 为 previous generation
8. 清理 stage / journal，释放锁
```

prepared journal 是 recovery 必须 roll-forward 的逻辑提交点：此前失败清理 stage 并保留完整旧 generation；此后任何 crash / cancel 都必须在下次 `read_consistent` 前完成 new generation，**NEVER** 向 reader 暴露部分 old + 部分 new。prepared 之后 adapter **NEVER** 返回 `Err`：若本次调用已完成 member publish，返回 `Visible` receipt；若剩余发布只能由 journal recovery 完成，返回 `RecoveryPending` receipt 与 warning。两种 receipt 都携 new revision、都表示 committed，领域 service 必须发布 candidate；后续外部 reader 先 recovery 再读取。只有 prepared 之前的 I/O / CAS 失败可以返回普通 `StorageError` 并让上层保留旧 live state。

上段“prepared 之后不返回 `Err`”约束适用于正常 I/O / crash recovery：它们 **MUST** 收敛为 committed receipt。若 journal 自身无效、已 fsync 的 staged member 丢失/摘要不符，或发布后的 member digest 与 journal 不一致，Storage 已无法证明完整 generation；此时 `read_consistent` / reopen **MUST** quarantine 整笔事务证据并返回 `StorageErrorKind::CorruptTransaction`（`InvalidJournal` / `DatasetMemberDigestMismatch`），**NEVER** 把它标成 `ConcurrentWrite`、普通 `Io`、`RecoveryPending` 或可用空 dataset。该错误表示“已提交事实无法安全物化”，不是 `NotCommitted`；上层 **MUST** 阻止 service 发布或开放，等待人工恢复。

`commit_atomic` 的 `members` 永远是新 generation 的完整清单：Storage 按 `DatasetManifest` 比较新旧成员名集合，新清单中缺席的旧成员名在同一 journal 内标记为 omitted=delete 并随 generation 切换一起物理删除，不允许残留孤儿文件，也不允许因为调用方“忘记带上”而静默保留旧成员。previous generation 是切换前完整旧成员集合的物理快照，和 blob 的 previous 一样只在启用恢复代际的 namespace 保留，可通过 `read_previous` / `promote_previous` / `quarantine` 显式访问、回滚或隔离；`read_consistent` 与 `read_manifest` 只服务当前 generation，**NEVER** 因为当前 generation 部分缺失就自动去读 previous 拼凑结果。

可执行 crash-state test **MUST** 在每个 stage / fsync / journal / member publish / committed-marker 点中断，再证明 reopen 只得到完整旧 generation 或完整新 generation。相同 primitive 同时服务 Memory active+archive 与 legacy key migration，**NEVER** 复制领域专属事务算法。

## 6. 机械读取与领域恢复

Storage 不判断 opaque bytes 是否符合领域 schema。恢复由数据 BC 驱动：

```text
consumer read Primary
  ├─ missing → consumer MAY read Previous
  ├─ decoder accepts → use primary
  └─ decoder rejects
       ├─ read Previous + decoder accepts → consumer requests promote_previous
       └─ both reject/missing → consumer requests quarantine + returns domain load error
```

`Generation::Primary/Previous` 都是机械 bytes。是否接受 payload、如何迁移旧 schema、是否自动 promote、如何提示用户或发 integration event，由数据 BC 决定。Storage 的 `quarantine` 只移动该 key 的物理代际并返回 receipt，不将 JSON 解析失败建模为通用 Storage 错误。

`promote_previous` 也遵循原子提交协议；成功后 Previous bytes 成为 Primary，损坏的原 Primary 进入 quarantine。Primary 缺失但 Previous 存在时，消费者同样可以验证后 promote，覆盖“提交边界外人工删除”等恢复场景。返回的 `WriteReceipt` 与 `write_atomic` 共用同一 committed+warning 语义：只要拿到 receipt 就必须发布新 primary，`warning` 表示某个提交后收尾步骤（例如旧 primary quarantine 归档、journal 清理）尚未完成，**NEVER** 因为出现 warning 而怀疑 promote 是否已提交。

## 7. 责任分配

| 关注点 | 所有者 |
|---|---|
| 原子写、fsync、replace、backup、quarantine | Storage |
| 多 member generation、dataset lock、journal / commit recovery、manifest 冻结、previous read/promote/quarantine | Storage `AtomicDatasetPort` |
| 物理根目录与后端 adapter | Storage + ConfigSnapshot 静态值 |
| Session / Memory schema | Context Management / Memory；Session 内嵌的 Task / Workspace DTO schema 仍分别由 Task / Project 发布 |
| schema version 与 migration | 对应数据 BC |
| 保存时机、turn-level save、级联删除 | 对应应用服务/数据 BC |
| Tool Result 的落盘阈值和 preview | Config 静态值 + Tool/Context Management 策略 |
| retention、compact、archive、eviction | 数据 BC；Storage 只执行明确命令 |
| Audit Event 的不可变语义与 retention policy | Audit；append-log 物理写入由 Audit adapter 以 file append detail 直接实现，只复用 Storage 路径安全 primitive |
| 日志 rotation/retention | Logging，不复用 Storage 业务端口 |

## 8. 生命周期与清理

Storage 可以提供 `delete_all_generations/list_primary` 等机械能力，但不得自行猜测数据是否过期。`list_primary` 永远隐藏 stage/previous/quarantine；`delete_all_generations` 幂等删除 primary、previous 及本 key 可识别的未提交临时文件，`DeleteOptions.include_quarantine` 决定是否一并删除 quarantine，默认 true 以兑现用户业务删除。若 quarantine 需保留取证，数据 BC 必须显式 opt out 并给出独立 retention 命令；不能让隐藏副本无限期遗留。

清理流程由拥有生命周期的 BC 发起：

- Context Management 删除 Session 后明确删除关联 snapshot/blob；
- Tool/Context Management 决定 Tool Result 是否成为孤儿并请求清理；
- Memory 决定归档与淘汰；
- Audit 决定审计 retention；
- Storage 只保证命令的路径安全、幂等性和失败可观察。

启动时对 `.tmp` 等未提交文件的清扫可以属于 Storage 机制，但只能识别本 adapter 自己的临时命名协议，不能删除未知文件。

## 9. Composition Root

Composition Root 负责：

- 从 ConfigSnapshot 取得各 namespace 的根目录、最低 durability 与代际策略；
- 构造 Storage 的 `AtomicBlobPort` / `AtomicDatasetPort` 文件系统 adapter；
- 构造依赖数据 BC Snapshot PL 的 integration adapter，并注入 Context Management、Memory 与 Audit；Task / Project 只向 Context Management 发布内嵌 snapshot，**NEVER** 获得独立 Storage adapter 或形成重复持久化路径；
- 确保领域 BC 不直接拼接 `~/.agents` 物理路径；
- **Config bootstrap 例外**：Config 的 FileAdapter 在 Storage 尚未按 ConfigSnapshot 装配前读取自身配置，是获准直接访问配置文件的 bootstrap adapter；Config 的 merge / validation / update 策略仍不得直接做 I/O，该例外不得扩散到其他 BC。
- Composition Root 保持测试中可替换为内存或临时目录 adapter。

## 10. 架构守卫目标

```text
Rule: storage-does-not-own-domain-models
Deny: Storage domain importing Session/Memory/Task/Workspace aggregates

Rule: domain-storage-through-ports
Deny: data capability policy / use-case code directly using fs::write/read/rename
Allow: Storage adapters and explicitly approved non-domain infrastructure

Rule: storage-paths-are-resolved-in-adapters
Deny: arbitrary absolute PathBuf crossing Storage PL
```

守卫不得阻止领域 BC 定义自己的 Snapshot 与 migration；它只约束物理 IO 和反向依赖。

## 11. 相关文档

- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Context Map 持久化边：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 摘要初稿：数据所有权、原子写/backup/quarantine 机制、窄端口与路径安全 | #793 |
| 2026-07-14 | 为 AtomicDataset 增加 expected-revision CAS 与 typed committed receipt，并移除 Task / Project 直连 Storage 路径 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 增加 typed CorruptTransaction + quarantine disposition，统一 blob / dataset digest 歧义的 fail-closed 恢复语义 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 移除 Storage 端 AtomicAppendLogPort（append-log 归 Audit-owned adapter 直接实现）；read() 取消跨代自动 fallback；补 dataset manifest + previous read/promote/quarantine；promote/quarantine 补齐 committed+warning receipt 与 generation/scope/reason | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | 冻结 §3 Storage Hexagonal 物理结构为 `domain + ports + adapters`：三个机制由 domain 子模块表达，不叠加 `capabilities/`；以稳定层名、单向依赖和窄 façade 支持机械 Guard，防止 I/O 下沉、adapter 泄漏与公开面劣化 | [#880](https://github.com/rushsinging/aemeath/issues/880) |
| 2026-07-16 | 冻结 blob generation policy 与幂等结果：namespace 静态选择 Retain/Discard；promote 区分 Promoted/AlreadyPromoted/NotFound；quarantine 区分 Moved/AlreadyAbsent，且跨 reopen promote 证据归 #882 journal | [#881](https://github.com/rushsinging/aemeath/issues/881) |
