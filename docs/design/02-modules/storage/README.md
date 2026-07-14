# Storage（通用域）

> 层级：02-modules / storage（模块摘要设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#793（S2）
> Storage 提供可靠的物理持久化机制，但不拥有 Session、Memory、Task、Workspace、History、Tool Result 或 Audit Event 的业务语义。

## 1. 模块定位

Storage 是数据 BC 与物理介质之间的机制边界：

```text
Context Management / Memory / Task / Project / Audit / Tool
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
3. **端口分两层归属**：Storage 拥有机械 `AtomicBlobPort` OHS；数据 BC 拥有 `SessionSnapshotStore` 等窄出站端口，其 integration adapter 位于消费方外层或 Composition，不让 Storage 反向依赖领域模型。
4. **原子可见**：一次替换后，读者只能看见完整旧值或完整新值，不能看见空文件或半截 payload。
5. **保留上一代物理完整值**：启用恢复代际的 namespace 在替换已有值时保留上一代完整 bytes；是否符合领域 schema 只能由数据 BC 验证。
6. **损坏不静默丢弃**：数据 BC 验证主值失败后可机械读取上一代，并显式请求 promote 或 quarantine；不得自动当作空数据继续。
7. **路径安全覆盖竞态**：除 segment 词法校验外，文件 adapter 必须使用受约束目录句柄、no-follow/create-new 或等价机制，防止 symlink 与 TOCTOU 越出根目录。
8. **业务策略不下沉**：阈值、preview、retention、级联删除、schema migration、compact/eviction 均留在拥有数据语义的 BC；Config 只提供静态默认值。
9. **无 Run checkpoint**：Storage 不建设 durable Run、Model Invocation checkpoint 或未完成 ToolCall 自动重放。

## 3. Published Language

以下签名表达语义，不锁定具体 Rust API：

```rust
struct StorageKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

struct WriteOptions {
    durability: Durability,
}

enum Durability { ProcessCrashSafe, BestEffort }
enum Generation { Primary, Previous }

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

enum ReadOutcome {
    Found(BlobRead),
    NotFound,
}

enum StorageErrorKind {
    InvalidKey,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    ConcurrentWrite,
}
```

`StorageKey` 表达逻辑位置，不暴露用户主目录或绝对路径。物理路径由 adapter 根据 ConfigSnapshot 提供的根目录与 namespace policy 解析。namespace policy 固定是否保留上一代；调用方不能逐次关闭该安全属性。

### 3.1 端口形态

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
    async fn promote_previous(&self, key: &StorageKey) -> Result<(), StorageError>;
    async fn quarantine(&self, key: &StorageKey) -> Result<QuarantineReceipt, StorageError>;
    async fn delete_all_generations(
        &self,
        key: &StorageKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError>;
    async fn list_primary(&self, prefix: &StorageKey) -> Result<Vec<StorageEntry>, StorageError>;
}
```

数据 BC 定义更窄端口，例如 `SessionSnapshotStore`、`MemorySnapshotStore`、`AuditEventStore`。这些端口的 integration adapter 依赖数据 BC Snapshot PL，并在内部调用 Storage 的 `AtomicBlobPort`；它位于消费方 adapter 层或 Composition，不进入 Storage BC。`AtomicBlobPort` 本身不携带 `Session`、`Task` 或 `MemoryEntry` 类型。

## 4. 原子写协议

目标文件 adapter 对启用上一代恢复的 namespace 采用可验证协议：

```text
1. 通过受约束根目录句柄解析 key；拒绝 symlink/no-follow 违规
2. 获取同 key 的进程内 + 跨进程写锁
3. create-new 随机 stage；write_all + file fsync
4. 若 primary 存在：以原子 link/copy-to-stage 方式生成 previous.next 并 fsync
5. 原子 replace stage → primary；fsync 父目录（提交点）
6. 提交后原子 replace previous.next → previous；再次 fsync 父目录
7. 清理本事务临时文件，释放锁，返回 WriteReceipt
```

提交点在第 5 步：此前 crash 保留旧 primary；此后 crash 保留新 primary。每个事务写入带 nonce 与 commit marker 的内部 journal：启动恢复若看到 commit marker + `previous.next`，必须完成 previous promotion；没有 commit marker 的 stage/previous.next 才可清理。由此 commit 后、previous 更新前的 crash 不会丢失直接上一代。`previous` 更新失败时写入返回 `CommittedWithBackupWarning`，journal 保留供下次恢复；不得谎称上一代已更新。首次写入没有 previous。所有 adapter 共享这张 crash-state 恢复表，不得自行选择清扫语义。

`ProcessCrashSafe` 表示 stage 文件和提交目录项都完成所需同步；`BestEffort` 只保证进程内原子可见。namespace 规定最低 durability，逐次 WriteOptions 只能提高，不能降低；平台无法兑现时返回 `UnsupportedDurability`。

### 4.1 关键不变量

- stage、primary 与 previous 位于同一文件系统；
- 临时名称不可预测且使用 create-new；
- 新 payload 同步完成前不得修改 primary；
- primary 在任何 crash point 都是完整旧值或完整新值；
- previous 只保存曾提交过的完整 primary bytes，但不承诺领域可解析；
- 同一 key 的进程内和跨进程写必须串行化；
- 成功或带 warning 返回时必须明确 committed/durability/previous 状态；
- 残留 stage/previous.next 不参与普通读取；启动恢复必须按 commit marker 完成或回滚事务；
- 所有 open/rename/delete 均相对受约束目录句柄执行，禁止跟随 symlink 越界。

## 5. 机械读取与领域恢复

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

`promote_previous` 也遵循原子提交协议；成功后 Previous bytes 成为 Primary，损坏的原 Primary 进入 quarantine。Primary 缺失但 Previous 存在时，消费者同样可以验证后 promote，覆盖“提交边界外人工删除”等恢复场景。

## 6. 责任分配

| 关注点 | 所有者 |
|---|---|
| 原子写、fsync、replace、backup、quarantine | Storage |
| 物理根目录与后端 adapter | Storage + ConfigSnapshot 静态值 |
| Session/Memory/Task/Workspace schema | 对应数据 BC |
| schema version 与 migration | 对应数据 BC |
| 保存时机、turn-level save、级联删除 | 对应应用服务/数据 BC |
| Tool Result 的落盘阈值和 preview | Config 静态值 + Tool/Context Management 策略 |
| retention、compact、archive、eviction | 数据 BC；Storage 只执行明确命令 |
| Audit Event 的不可变语义与 retention policy | Audit；物理写入可经 Storage |
| 日志 rotation/retention | Logging，不复用 Storage 业务端口 |

## 7. 生命周期与清理

Storage 可以提供 `delete_all_generations/list_primary` 等机械能力，但不得自行猜测数据是否过期。`list_primary` 永远隐藏 stage/previous/quarantine；`delete_all_generations` 幂等删除 primary、previous 及本 key 可识别的未提交临时文件，`DeleteOptions.include_quarantine` 决定是否一并删除 quarantine，默认 true 以兑现用户业务删除。若 quarantine 需保留取证，数据 BC 必须显式 opt out 并给出独立 retention 命令；不能让隐藏副本无限期遗留。

清理流程由拥有生命周期的 BC 发起：

- Context Management 删除 Session 后明确删除关联 snapshot/blob；
- Tool/Context Management 决定 Tool Result 是否成为孤儿并请求清理；
- Memory 决定归档与淘汰；
- Audit 决定审计 retention；
- Storage 只保证命令的路径安全、幂等性和失败可观察。

启动时对 `.tmp` 等未提交文件的清扫可以属于 Storage 机制，但只能识别本 adapter 自己的临时命名协议，不能删除未知文件。

## 8. Composition Root

Composition Root 负责：

- 从 ConfigSnapshot 取得各 namespace 的根目录、最低 durability 与代际策略；
- 构造 Storage 的 `AtomicBlobPort` 文件系统 adapter；
- 构造依赖各数据 BC Snapshot PL 的 integration adapter，并注入 Context Management、Memory、Task、Project 与 Audit；
- 确保领域 BC 不直接拼接 `~/.agents` 物理路径；
- **Config bootstrap 例外**：Config 的 FileAdapter 在 Storage 尚未按 ConfigSnapshot 装配前读取自身配置，是获准直接访问配置文件的 bootstrap adapter；Config application/domain 仍不得直接做 IO，该例外不得扩散到其他 BC。
- Composition Root 保持测试中可替换为内存或临时目录 adapter。

## 9. 架构守卫目标

```text
Rule: storage-does-not-own-domain-models
Deny: Storage domain importing Session/Memory/Task/Workspace aggregates

Rule: domain-storage-through-ports
Deny: data BC application/domain code directly using fs::write/read/rename
Allow: Storage adapters and explicitly approved non-domain infrastructure

Rule: storage-paths-are-resolved-in-adapters
Deny: arbitrary absolute PathBuf crossing Storage PL
```

守卫不得阻止领域 BC 定义自己的 Snapshot 与 migration；它只约束物理 IO 和反向依赖。

## 10. 相关文档

- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Context Map 持久化边：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 摘要初稿：数据所有权、原子写/backup/quarantine 机制、窄端口与路径安全 | #793 |
