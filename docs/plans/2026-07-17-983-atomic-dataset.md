# #983 AtomicDataset 实施计划

## 目标

在 Storage 现有 `domain + ports + adapters` 六边形结构中增加独立的多 member dataset 事务，不组合 `AtomicBlobPort`，以 Prepared journal 为逻辑提交点，实现完整 generation、opaque revision CAS、Previous 与 typed crash recovery。

## 文件边界

- `src/domain/atomic_dataset.rs`：DatasetKey、DatasetMember、DatasetRevision、DatasetManifest、DatasetRead/Outcome、DatasetCommitReceipt/Visibility，以及规范排序、revision 与纯恢复决策。
- `src/ports/atomic_dataset_port.rs`：AtomicDatasetPort OHS。
- `src/adapters/dataset_protocol.rs`：adapter-private manifest/journal schema、digest、协议文件名与编解码。
- `src/adapters/dataset_filesystem.rs`：capability-relative 路径、dataset lock、CAS、stage、Prepared、逐 member roll-forward、previous/promote/quarantine。
- `tests/atomic_dataset_contract.rs`：只经公共 API 的共享契约。
- `tests/atomic_dataset_crash.rs`：协议故障矩阵、证据矛盾、真实子进程 abort/lock。

## 执行顺序

1. L1 RED：为 DatasetKey、重复 member、规范排序、空 revision、顺序无关、事实敏感、omitted 与恢复决策写失败测试。
2. L1 GREEN：实现 domain PL 和纯规则；不包含 fs/path/journal schema。
3. L3 RED：写公共 AtomicDatasetPort contract，覆盖首次空 dataset、完整替换、omitted、CAS、Previous、promote/quarantine。
4. Port + façade：新增 trait 和最小受控 re-export；不公开 adapter-private schema/fault seam。
5. Adapter GREEN：实现 dataset 独立文件协议，使基础契约通过。
6. Crash RED：覆盖 stage/fsync/Previous/Prepared/逐 member publish/omitted delete/Committed/promotion/cleanup；每点 reopen 只得完整旧或新 generation。
7. Crash GREEN：Prepared 后普通故障转 committed RecoveryPending；读取入口机械 roll-forward；证据矛盾整笔 quarantine 并返回 typed corruption。
8. L5：真实 OS lock 与 Prepared/中间 publish 后 abort-reopen。
9. 验证：storage tests、fmt、production reachability、all-target clippy、workspace tests、coverage、architecture guards、public surface/source guard。
10. 独立 review 后回填 issue 文档与证据，提交并创建 PR。

## 核心不变量

- CAS 失败和重复 member 必须发生在创建任何 stage/journal 前。
- commit members 是完整 generation；遗漏旧 member 即事务内删除。
- Prepared durable 之前 Err=NotCommitted；之后普通故障只返回 committed receipt。
- Prepared 后只 roll-forward，任何读取先恢复，永不暴露混代。
- journal/member 证据矛盾优先 typed CorruptTransaction，不伪装 warning。
- Previous 是完整旧 manifest + member 集，Primary/Previous 均禁止隐式跨代拼接。
- 测试 fault seam 不进入生产 public surface；Guard/白名单净增 0。
