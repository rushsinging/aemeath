# Issue #880 StorageKey、SafePath 与 AtomicBlobPort 根因方案实施计划

> **执行要求：** 实施时使用 `superpowers:executing-plans` 或 `superpowers:subagent-driven-development`，严格按 TDD 的 Red → Green → Refactor 顺序推进。

**日期：** 2026-07-13  
**对应 Issue：** [#880](https://github.com/rushsinging/aemeath/issues/880)  
**父 Issue：** [#848](https://github.com/rushsinging/aemeath/issues/848) → [#845](https://github.com/rushsinging/aemeath/issues/845) → [#743](https://github.com/rushsinging/aemeath/issues/743)  
**目标分支：** `release/v0.1.0`  
**实施分支：** `refactor/880-storage-atomic-blob`  
**基线：** `release/v0.1.0@d62b10f9`

## 1. 目标

建立 Storage BC 第一段可生产复用的纯机制边界：

```text
领域 Snapshot/Blob
  → StorageKey（逻辑位置）
  → AtomicBlobPort（opaque bytes）
  → FileAtomicBlobAdapter（受约束目录句柄）
  → 同目录随机 stage + 原子 replace
```

本 Issue 完成后应满足：

- 跨 BC 只交换 key、bytes、options、outcome、receipt 和结构化错误；
- `StorageKey` 无法表达绝对路径、父目录、空段或分隔符注入；
- 文件 adapter 不接收任意 `PathBuf`，所有 IO 都相对初始化时取得的 capability root；
- stage 使用不可预测名称和 create-new，在同一目录/文件系统内提交；
- `write_atomic` 的 replace 线性化点之前失败时 primary 保持完整旧值，replace 成功后读取到完整新值；
- Storage 不解析 JSON，不拥有 Session、Task、Memory 等领域类型；
- API 为 #869、#881、#883、#884 提供基础机制，但 generation/durability 扩展由后续 Issue 发布。

## 2. 根因与方案选择

当前 Session、Memory、History、Tool Result 各自拼接物理路径并直接读写，导致原子协议、错误分类和路径约束重复且不一致。只在现有调用点增加 `.tmp + rename` 属于止血：它仍允许任意路径越界，无法统一 symlink/TOCTOU 防护，也会继续复制持久化机制。

采用根因方案：

1. 用不透明值对象在构造期拒绝非法路径表达；
2. 用 capability-oriented 目录句柄把物理 root 固定在 adapter 内；
3. 用 Storage-owned port 发布纯机制语言；
4. 用同目录随机 stage、create-new 和平台原子 replace 建立线性化点；
5. 用结构化错误保留调用操作和安全分类，不把领域解码错误伪装成 Storage 错误。

计划在当前受支持的 Unix 平台（Linux CI 与 macOS release target）使用目录句柄相对操作。优先评估 `cap-std` 作为 capability root；逐段 no-follow、同父目录原子替换若不能由其安全表达，则在 adapter 私有层使用 `rustix` 的 `openat/renameat` 等原语。不得使用 `canonicalize + starts_with` 或“检查 metadata 后按字符串重新打开”，因为两者存在检查后替换竞态。Windows 当前不是构建目标；未来支持时必须先实现并验证真正的 replace primitive，不得降级为先删除再 rename。

## 3. 范围边界

### 本计划包含

- `StorageNamespace`、`SafePathSegment`、`StorageKey`；
- `WriteOptions`（本期只含原子写所需选项）、`BlobRead`、`ReadOutcome`；
- `WriteReceipt`、`StorageErrorKind`、`StorageOperation`、`StorageError`；
- 异步 `AtomicBlobPort::read/write_atomic`；
- capability-safe 文件 adapter；
- Primary round-trip、覆盖、路径穿越、symlink、随机 stage、旧值/新值二选一测试；
- contract/gateway 导出和使用示例测试。

### 本计划不包含

- Previous、generation-aware read、promote、quarantine：#881；
- 跨进程锁、journal、commit marker、file/directory sync、durability policy/capability detection、完整 crash-point fault injection：#882；
- Task/Memory 模型迁出和消费者切换：#883；
- AppendLog、Tool Result blob、History 瘦身：#884；
- Session schema、legacy reader 和自动迁移：#869；
- 任意领域 payload 的 JSON 编解码或验证；
- 全局架构 Guard：#763。

本次 `read` 只表达当前 primary，不发布半成品 `Generation` 枚举；#881 在同时具备 Primary/Previous 真实语义时扩展为 generation-aware read。#880 的“旧值或新值二选一”仅指单次 Primary replace 的原子可见性，不等同于 crash 后 Previous 恢复。

## 4. 目标文件结构

```text
agent/features/storage/src/
├── contract.rs
├── contract/
│   ├── atomic_blob.rs
│   ├── error.rs
│   └── storage_key.rs
├── gateway.rs
└── gateway/
    ├── file_atomic_blob.rs
    └── file_atomic_blob/
        ├── commit.rs
        ├── path.rs
        └── tests.rs
```

现有 `api.rs` 继续聚合 `contract::*` 与 `gateway::*`。不得把文件 adapter 类型放进 `contract`，也不得让 `business::{task,memory}` 依赖新 adapter；消费者迁移留给后续叶子 Issue。

## 5. Published Language 决策

### 5.1 Key

- `StorageNamespace` 为封闭枚举，本期明确包含 `Sessions`、`Memory`、`Tasks`、`History`、`ToolResults`、`Audit`；只决定固定逻辑目录，不携带 durability policy。
- `SafePathSegment::new` 拒绝空串、`.`、`..`、NUL、`/`、`\\`、绝对路径与平台 prefix；内部字符串私有。当前 Unix 实现还拒绝无法安全映射的特殊文件名；未来 Windows 支持前必须另补 `:`/ADS、设备名、尾点/空格和大小写别名策略测试。
- `StorageKey::new(namespace, segments)` 至少需要一个 segment；提供只读 accessor 和 `child`，不提供任意 `PathBuf` 转换。
- key 的 `Debug/Display` 只显示逻辑 namespace/segment，不泄漏 home 绝对路径。

### 5.2 Port

`AtomicBlobPort` 只发布两个本期有真实语义的方法：

- `read(key) -> Result<ReadOutcome, StorageError>`；
- `write_atomic(key, bytes, options) -> Result<WriteReceipt, StorageError>`。

`ReadOutcome::NotFound` 是正常机械结果；权限和不安全文件系统条目是错误。空 bytes 是合法 blob，不得等同 NotFound。`WriteOptions` 本期不包含 durability；#882 在有真实同步实现和能力检测时扩展。

### 5.3 WriteReceipt

`WriteReceipt` 只表达本次调用成功越过原子 replace 线性化点，不提供 `replaced_existing`、previous 状态或 durability。缺少同 key 锁时，这些字段无法可靠计算；它们必须随 #881/#882 的代际和并发协议一起加入。

### 5.4 Error

`StorageKeyError` 独立表达 key 构造失败。已验证 `StorageKey` 进入 port 后，`StorageError` 保存 `kind + operation + logical key + source`。`StorageOperation` 本期只发布稳定的 `Read`、`WriteAtomic`；stage/open/replace 等内部阶段留在 source context。`StorageErrorKind` 本期只区分真实可产生的：

- `Io`；
- `PermissionDenied`；
- `UnsupportedAtomicReplace`；
- `UnsafeFilesystemEntry`（symlink/非预期文件类型）。

错误枚举标记为可扩展；`ConcurrentWrite`、`UnsupportedDurability` 在 #882 有真实锁和能力检测时加入。错误的用户可见文本使用中文；底层 source 保留诊断链。不得把 serde、Session corrupt 或 Memory validation 纳入该枚举。

## 6. 分阶段实施任务

### Task 1：验证 capability 与 atomic replace 原语

**Files:**
- Modify: `Cargo.toml`
- Modify: `agent/features/storage/Cargo.toml`
- Modify: `Cargo.lock`

**Steps:**
1. 增加 `async-trait`、`thiserror`、`cap-std`、Unix 私有 adapter 所需的 `rustix`；测试依赖增加 `tempfile`。
2. 编写临时编译 spike，确认：基于已打开父目录句柄逐段 no-follow open、create-new、同目录 rename-over-existing、remove 和 read 可实现。
3. 在 Linux/macOS 分别记录采用的 primitive；无法保证 replace 时映射为 `UnsupportedAtomicReplace`，禁止先删后 rename。
4. 删除 spike，只保留正式依赖。

**Verify:** `cargo check -p storage`。

### Task 2：为 SafePathSegment 写失败测试

**Files:**
- Create: `agent/features/storage/src/contract/storage_key.rs`
- Modify: `agent/features/storage/src/contract.rs`

**Steps:**
1. 表驱动测试普通 ASCII、Unicode和带点文件名可构造。
2. 表驱动测试空串、`.`、`..`、NUL、斜杠、反斜杠和绝对路径被拒绝。
3. 测试错误包含稳定 reason，不包含物理 root。
4. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_safe_path_segment_validation`，首次预期 FAIL。

### Task 3：实现 SafePathSegment

**Files:**
- Modify: `agent/features/storage/src/contract/storage_key.rs`

**Steps:**
1. 实现私有字符串和 checked constructor。
2. 实现只读字符串 accessor、Eq/Hash/Clone。
3. 实现独立 `StorageKeyError`，不依赖 adapter error。
4. 不实现 `From<PathBuf>`、`AsRef<Path>` 或未校验字符串转换。

**Verify:** Task 2 测试 PASS。

### Task 4：为 StorageNamespace 与 StorageKey 写失败测试

**Files:**
- Modify: `agent/features/storage/src/contract/storage_key.rs`

**Steps:**
1. 表驱动锁定六个 namespace 的稳定逻辑目录名。
2. 测试空 segments 被拒绝。
3. 测试 `child` 只能接收已验证 segment。
4. 测试 Debug/Display 不泄漏绝对物理路径。
5. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_storage_key_invariants`，首次预期 FAIL。

### Task 5：实现 StorageNamespace 与 StorageKey

**Files:**
- Modify: `agent/features/storage/src/contract/storage_key.rs`

**Steps:**
1. 实现六个封闭 namespace variants 和唯一目录映射。
2. 实现 key checked constructor、只读 accessors、`child`、Eq/Hash/Clone。
3. 保持物理 root 与 `PathBuf` 不进入 Published Language。

**Verify:** Task 4 测试 PASS；`cargo test -p storage contract::storage_key::tests` PASS。

### Task 6：为 AtomicBlobPort Published Language 写失败测试

**Files:**
- Create: `agent/features/storage/src/contract/atomic_blob.rs`
- Create: `agent/features/storage/src/contract/error.rs`
- Modify: `agent/features/storage/src/contract.rs`

**Steps:**
1. 用内存 fake 测试 NotFound、空 blob、普通 bytes。
2. 编译测试 `Arc<dyn AtomicBlobPort>` 对象安全性。
3. 测试 `StorageOperation` 只有 Read/WriteAtomic，receipt 不声称 existing/durability/previous 状态。
4. 测试 error kind、operation、logical key、source 保留和中文 Display。
5. 分别运行两个目标测试确认 Red。

**Verify:**
- `cargo test -p storage test_atomic_blob_port_object_safe`，首次预期 FAIL。
- `cargo test -p storage test_storage_error_preserves_public_context`，首次预期 FAIL。

### Task 7：实现 AtomicBlobPort Published Language

**Files:**
- Modify: `agent/features/storage/src/contract/atomic_blob.rs`
- Modify: `agent/features/storage/src/contract/error.rs`

**Steps:**
1. 实现 options、outcome、blob read 和最小 receipt。
2. 实现可扩展结构化错误与 source 链。
3. 定义 `AtomicBlobPort: Send + Sync` 的 `read/write_atomic`。
4. 所有 payload 只使用 bytes，禁止 serde 泛型和领域类型。

**Verify:** Task 6 两个测试 PASS；`cargo check -p storage` PASS。

### Task 8：为首次写入写失败测试

**Files:**
- Create: `agent/features/storage/src/gateway/file_atomic_blob.rs`
- Create: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`
- Modify: `agent/features/storage/src/gateway.rs`

**Steps:**
1. 使用临时 root fixture，测试 primary 不存在时 read 返回 NotFound。
2. 测试首次写入普通 bytes、空 bytes、非 UTF-8 bytes 均 round-trip。
3. 测试结束后不存在本事务 stage 残留。
4. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_write_atomic_first_value_round_trips`，首次预期 FAIL。

### Task 9：实现首次写入

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob.rs`
- Create: `agent/features/storage/src/gateway/file_atomic_blob/commit.rs`

**Steps:**
1. adapter 构造时取得 ambient root 一次并转换为 capability root。
2. 在目标父目录内生成随机 stage，以 create-new 打开并完整写入。
3. 通过同一已打开父目录句柄 rename stage → primary。
4. 成功后返回最小 receipt；失败时 best-effort 删除本事务 stage，清理错误不覆盖原始错误。
5. 将阻塞 IO 收敛到单一 `spawn_blocking` seam；文档注明取消 future 不等于取消已启动的文件事务。

**Verify:** Task 8 测试 PASS。

### Task 10：为 stage 名碰撞写失败测试

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`

**Steps:**
1. 通过私有 `StageNameSource` fake 固定连续两个相同 nonce，再提供新 nonce。
2. 预建同名 stage，断言 adapter 以 create-new 检测碰撞并重试，不截断攻击者文件。
3. 断言最终 primary 正确，预建碰撞文件内容不变。
4. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_stage_collision_never_truncates_existing_file`，首次预期 FAIL。

### Task 11：实现可控 StageNameSource

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob.rs`
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/commit.rs`

**Steps:**
1. 生产默认实现使用 UUID nonce。
2. 测试构造入口注入确定性 name source，保持类型私有或 `cfg(test)`。
3. 对 AlreadyExists 只重取 nonce，不使用 truncate/open-existing。
4. 设置有限重试上限，耗尽后返回结构化 IO error。

**Verify:** Task 10 测试 PASS。

### Task 12：为覆盖原子可见性写失败测试

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`

**Steps:**
1. 先写入带校验模式的旧 payload。
2. 用 commit barrier 确保 reader 与 replace 确实重叠，再写不同长度的新 payload。
3. reader 每次只允许完整旧 payload 或完整新 payload，禁止空值、半截值和混合值。
4. 运行目标测试确认 Red，不以“连续跑若干次”替代确定性 barrier。

**Verify:** `cargo test -p storage test_overwrite_exposes_only_complete_old_or_new_value`，首次预期 FAIL。

### Task 13：实现平台原子覆盖

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/commit.rs`

**Steps:**
1. Unix 使用同一已打开父目录句柄上的原子 rename-over-existing primitive。
2. 禁止 remove-primary 再 rename。
3. 保留私有 commit barrier seam，让测试精确控制 replace 前后时序。
4. 不在本期执行 file/directory sync，也不声明 process-crash durability。

**Verify:** Task 12 测试 PASS；Linux CI 与 macOS release 构建均使用相同语义测试。

### Task 14：为 pre-replace 故障写失败测试

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`

**Steps:**
1. 先写完整旧 primary。
2. 注入 stage 写完、replace 之前的确定性失败。
3. 断言调用返回 WriteAtomic error，primary 仍是完整旧值。
4. 断言本事务 stage 已清理或被明确识别，不参与普通读取。
5. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_failure_before_replace_preserves_primary`，首次预期 FAIL。

### Task 15：实现 pre-replace fault seam

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/commit.rs`

**Steps:**
1. 增加私有 fault injector，只允许在明确 commit point 前触发。
2. 将 replace 定义为本期唯一线性化点。
3. 错误文本和测试明确：只承诺 replace 前故障保留旧值；replace 后进程终止或返回路径由 #882 journal/durability 协议处理。

**Verify:** Task 14 测试 PASS。

### Task 16：为 no-follow 目标文件写失败测试

**Files:**
- Create: `agent/features/storage/src/gateway/file_atomic_blob/path.rs`
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`

**Steps:**
1. 在 primary 位置放置指向 root 外文件的 symlink。
2. 分别调用 read 和 write，断言 `UnsafeFilesystemEntry`。
3. 断言 root 外目标内容不变。
4. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_target_symlink_is_never_followed`，首次预期 FAIL。

### Task 17：实现目标文件 no-follow

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/path.rs`
- Modify: `agent/features/storage/src/gateway/file_atomic_blob.rs`

**Steps:**
1. 基于已打开父目录句柄执行最终 open/read/replace，不按绝对路径重新打开。
2. 最终读取使用 `O_NOFOLLOW` 或平台等价原语。
3. rename 前不得依赖单独 metadata 检查来宣称安全；目标类型检查和 replace 必须使用 handle-relative 原语。

**Verify:** Task 16 测试 PASS。

### Task 18：为中间目录竞态写失败测试

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/tests.rs`

**Steps:**
1. 创建 root 内合法祖先目录和 root 外诱饵目录。
2. 通过 path-resolution barrier，在取得某一级父句柄后重命名/替换其路径名为指向 root 外的 symlink。
3. 继续 read/write，断言操作保持绑定已打开目录对象或安全失败，绝不访问 root 外诱饵。
4. 运行目标测试确认 Red。

**Verify:** `cargo test -p storage test_ancestor_swap_cannot_escape_capability_root`，首次预期 FAIL。

### Task 19：实现逐段 handle-relative 路径解析

**Files:**
- Modify: `agent/features/storage/src/gateway/file_atomic_blob/path.rs`

**Steps:**
1. 从 capability root 开始逐段用 `O_NOFOLLOW|O_DIRECTORY` 或平台等价原语打开目录。
2. 后续 open/create/rename 始终相对已打开父目录句柄。
3. 禁止 `symlink_metadata → 字符串 open` 和 `canonicalize → starts_with`。
4. 缺失目录创建后立即以 no-follow 重新取得句柄；并发替换时安全失败。

**Verify:** Task 18 测试 PASS；Task 16 回归 PASS。

### Task 20：收口导出与边界

**Files:**
- Modify: `agent/features/storage/src/contract.rs`
- Modify: `agent/features/storage/src/gateway.rs`
- Verify: `agent/features/storage/src/api.rs`

**Steps:**
1. contract 只导出 PL 和 port；gateway 只导出 adapter 构造类型。
2. 增加从 `storage::api` 构造 key、注入 trait object、使用 temp adapter 的编译测试。
3. 用 compile-fail doctest 或签名断言证明 port 方法不接收逐次 root/任意 `PathBuf`。
4. 搜索新增代码，确认未 import Session/Task/Memory schema，未公开物理 root 或平台句柄。
5. 记录 #881/#882/#883 接续点，不新增兼容 facade。

**Verify:** `cargo test -p storage` PASS。

### Task 21：执行完整验证门禁

**Steps:**
1. `cargo fmt --all -- --check`。
2. `cargo test -p storage`。
3. `cargo clippy -p storage --all-targets -- -D warnings`。
4. `cargo check --workspace`。
5. `cargo test --workspace`。
6. `cargo clippy --workspace --all-targets -- -D warnings`。
7. `git diff --check`。
8. 搜索 `PathBuf|std::fs|serde_json|Session|Task|Memory` 在新增 contract/adapter 中的命中并逐项审计。

每条命令单独执行并保留结果；失败时先修当前门禁，不跳过或弱化断言。

### Task 22：审查、同步与创建实现 PR

**Steps:**
1. 调用 `superpowers:requesting-code-review`，重点审查 capability 边界、symlink/TOCTOU、atomic replace、取消/线性化点声明和 PL 泄漏。
2. 修正意见后重跑 Task 21。
3. 创建 PR 前执行 `git pull origin release/v0.1.0`，有冲突则解决后重跑 Task 21。
4. PR base 固定为 `release/v0.1.0`，正文使用 `Closes #880` 并列出 #881/#882 未包含范围。
5. 不自动合并、不自动关闭 #880；合并后按用户确认同步 #848、#845、#743 与 Release Gate #579。

## 7. 验收矩阵

| 场景 | 预期 |
|---|---|
| 合法多段 key | 稳定映射到 capability root 内 |
| `..`、绝对路径、分隔符、NUL | 构造期 `StorageKeyError` |
| 中间目录 symlink 指向 root 外 | 拒绝，外部内容不变 |
| 目标文件 symlink | 拒绝，不跟随覆盖 |
| primary 不存在 | `ReadOutcome::NotFound` |
| primary 为零字节 | `Found`，bytes 为空 |
| 首次写入 | 完整新值，receipt 仅表示调用成功越过 replace 线性化点 |
| 覆盖写入 | reader 只见完整旧值或新值 |
| stage 写后 replace 前失败 | primary 保持旧值 |
| replace 后进程终止/调用取消 | 本期不承诺 durable/回滚；由 #882 journal 与 durability 处理 |
| 非 UTF-8 payload | 原样 round-trip |
| 平台缺少原子覆盖 primitive | `UnsupportedAtomicReplace`，不降级为先删后 rename |
| IO/权限失败 | 结构化 kind + operation + logical key |
| Storage contract 依赖 | 不出现领域 schema、serde 解码或物理 PathBuf |

## 8. 风险与控制

1. **capability root 不等于自动拒绝全部 symlink：** 必须使用逐段 no-follow、handle-relative 原语，并以目标 symlink 和祖先交换竞态测试证明。
2. **rename 在平台间语义不同：** 本期只承诺已验证的 Linux/macOS；commit 层隔离平台实现，禁止“先删后改名”。
3. **durability 被误前移：** #880 只建立原子可见的 replace 线性化点；file/directory sync、journal、锁和能力检测全部留给 #882。
4. **async trait 内阻塞 runtime：** 所有文件 IO 收敛到单一 blocking seam，并明确 future 取消不能撤销已启动的文件事务。
5. **#880 与 #881/#882 越界：** 本期不加入 generation、previous、journal、durability 或锁的伪实现，只保留可扩展私有 seam。
6. **测试只验证最终值：** 必须用 barrier/fault injector 覆盖攻击竞态、pre-replace 失败和 reader/writer 真正重叠。
7. **公开 API 过宽：** contract 不公开 adapter internals、physical root 或平台句柄。

## 9. 完成定义

只有同时满足以下条件，#880 才进入待 review：

- StorageKey 不能表达越界路径；
- symlink/TOCTOU 攻击测试证明 adapter 不能逃逸 root；
- AtomicBlobPort 仅交换 opaque bytes 和机械 PL；
- Primary 写入采用同目录随机 create-new stage 和已验证平台的原子 replace；
- reader 在覆盖过程中只观察到完整旧值或新值；
- replace 前故障保持旧值，replace 后取消/crash 不做虚假回滚或 durability 承诺；
- Storage contract 不依赖任何领域 schema；
- storage 定向测试、workspace test/check/clippy 全部通过；
- 实现 PR 以 `release/v0.1.0` 为 base 创建并等待用户 review。
