# #1479 Canonical Session 分片增量 writer 实施计划

> 对应 Issue：#1479 Canonical Session 分片增量 writer
> 基线：`origin/main@3df33f26`
> 分支：`feature/1479-incremental-session-writer`

## 目标与边界

本计划只治理 Canonical Session 的读取、编码和写入峰值：Context 负责 Session 领域 manifest 与变更集，Storage 负责 generation 成员复用、CAS、原子提交和恢复，Composition 负责唯一装配。

不改 #1478 的 Resume/TUI backing、窗口索引和按需物化；不实现 compact 语义本身；不在 Storage domain 解释 Session 业务字段；不保留第二套生产 Resume DTO。

## 已确认的根因

当前 `CanonicalSessionRepository` 在多类 mutation 中执行 `(*current).clone()`，随后 `CanonicalSessionWriter::save(&candidate)`；`AtomicBlobCanonicalSessionWriter` 再执行 `SessionCodec::encode(session) -> Vec<u8>` 并通过 `AtomicBlobSessionStore::write` 完整轮换单 Blob。现有 `AtomicDatasetPort` 已有完整代 crash-safe 协议，但 `commit_atomic` 只接受完整 `DatasetMember` 字节，无法复用 primary 中未变化 member。

## 唯一推荐架构

### Context 领域

- `SessionGenerationManifest`：只描述 Session schema、revision、compact marker、metadata/snapshot member 引用和有序 Step identity。
- `SessionChangeSet`：明确列出新增/替换 member、复用 member、删除 member 以及新领域 revision。
- finalized Step member 不可变；Step identity 稳定，修改只生成新 member。
- canonical 内存状态使用结构共享；变更集生成只复制受影响的 member/metadata，不复制整个 Session。
- 旧单 Blob 仅由兼容 reader 读取；成功读取后一次性转换为新 generation。

### Storage domain / port

- 增加“新字节 member”和“复用既有 member”的窄类型，不使用宽泛 `Projection` 命名。
- 增量提交必须在同一 dataset lock 内验证 expected revision、复用 member 的 name/digest/length、重复名、缺失 member 和 unsafe path。
- primary/previous 联合可达；Prepared 之后只能前滚。
- GC 只能删除不被 primary、previous 或未完成事务引用的 orphan member。
- 旧完整 `commit_atomic` 可作为迁移兼容入口，但新 Session writer 不得使用它。

### Adapter

- stage 只写新增/替换 member。
- 复用 member 通过不可变文件复用；禁止对被复用内容原地修改。
- 先 durable 新 member，再写 Prepared journal，再发布 manifest。
- 任意 Prepared 后故障均恢复到完整新 generation；Prepared 前故障不跨越逻辑提交点。
- 保持 primary/previous、CAS、promote、quarantine、future-schema fail-closed 语义。

### Composition

- 生产 Session reader/writer 使用 AtomicDataset 能力。
- 旧 AtomicBlob reader 只用于兼容读取和一次性迁移。
- `CanonicalSessionWriter::save(&CanonicalSession)` 不再作为生产写入入口。
- `AtomicBlobCanonicalSessionWriter` 退役；工具 receipt ledger 若仍需单独持久化，保持独立边界，不将其混入 Session generation。

## TDD 执行顺序

每项先写失败测试，再实现生产代码；测试名称表达“条件 + 行为 + 结果”。

1. Storage domain：定义新字节/复用 member、CAS、重复/缺失/摘要冲突语义。
2. Storage adapter：定义复用不重新读取/写入历史 member、Prepared 前后故障、previous/promote、orphan GC 和 symlink 安全。
3. Context wire：定义新 Session manifest/member round-trip、旧 v1-v6 单 Blob迁移、future schema 原字节保留、primary/previous 恢复。
4. Context change set：定义 accepted input、finalize、tool receipt、compact、skill load、task/workspace snapshot 各自只产生变化 member，并保持领域 revision/幂等/冲突语义。
5. Composition：定义唯一 dataset 装配、legacy reader 只读迁移和 #1478 Resume 回归。
6. 退役守卫：拒绝生产路径调用完整 Session clone/save/encode，并检查无旧旁路和死代码。

## 验证门禁

- `cargo fmt --all -- --check`
- `cargo test -p storage --lib`
- `cargo test -p storage --tests`
- `cargo test -p context --lib`
- `cargo test -p context --tests`
- `cargo test -p composition --tests`
- `cargo test --workspace --all-targets --no-fail-fast`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `bash .agents/hooks/check-architecture-guards.sh`
- pre-push 完整 hook

固定大 Session 还需记录：变化 member 数、复用 member 数、写入字节数、decode/encode 峰值、live heap、allocation 数、primary/previous 恢复耗时。没有现场数据时只能报告“代码与契约验证通过”，不能宣称内存指标已改善。

## 最小补丁与根因方案取舍

最小补丁是固定成员拆分但每次重建完整 generation：成本低、复用现有 adapter，但写入和内存仍随完整历史线性增长，只能降低单 Blob 连续 buffer 峰值。

本计划选择根因方案：增量 change set + generation member 复用 + 结构共享。实现成本和迁移风险较高，需要完整 crash/future-schema/GC 测试，但能真正消除普通 Session mutation 的整批 clone/encode/write 放大链路，优先实施。
