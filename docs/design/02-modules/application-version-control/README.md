# Application Version Control（通用域）

> 层级：02-modules / application-version-control（模块摘要设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#793（S2）
> 本模块管理 aemeath 应用自身的版本发现、更新渠道、检查策略、可信 artifact 验证和自更新安装；不管理 Git 分支/tag、Cargo 依赖、milestone 或 release PR 流程。

## 1. 模块定位

```text
CLI / TUI
   │ AgentClient Application Control（入站 PL）
   ▼
Agent Runtime Application Control Router
   │ ApplicationVersionPort（Runtime 出站端口）
   ▼
Application Version Control App Service
   ├── Version Check Use Case
   ├── Update Apply Use Case
   ├── Channel + Cache Policy
   ├── Release Source ACL
   ├── Verified Update Plan
   └── Install Transaction
          │ 内部 source/cache/installer ports
          ▼
GitHub Releases / Signed Manifest / Platform Installer
```

CLI/TUI 仍只依赖 `AgentClient`。Runtime 的 Application Control Router 把检查/安装命令经 `ApplicationVersionPort` 交给本模块；`VersionCheckPort` 与 `UpdateApplyPort` 是模块 App Service 内部用例边界/协作端口，不直接暴露给 Runtime 或交付层。

Config 拥有默认渠道、是否启动检查和检查间隔等静态值；Config PL 使用自身稳定枚举/字符串表示，经 Composition/配置 ACL 转换为本模块 `UpdateChannel`，Config 不反向依赖本模块领域类型。Application Version Control 拥有版本比较、渠道筛选、检查缓存语义和升级策略。

## 2. 核心决策

1. **应用版本不是 Project Git 版本**：本模块只管理正在运行的 aemeath 可执行程序；worktree、branch、tag 和项目依赖升级属于其他边界。
2. **渠道是领域值对象**：`Stable` 与 `Prerelease` 必须在 ConfigSnapshot、检查请求、缓存键和 Release Source ACL 中保持同一语义，禁止自由字符串散落。
3. **检查与安装分端口**：版本检查是安全、可后台降级的查询；安装会修改当前可执行文件，是显式用户动作和独立事务。
4. **普通检查与强制检查语义不同**：普通检查遵守 TTL、rate-limit 和 stale fallback；强制检查跳过 TTL，但仍遵守网络安全与上游 rate-limit 事实。
5. **Release Source 通过 ACL**：GitHub JSON、tag、asset URL、header 和 rate-limit 字段不得直接成为模块 PL。
6. **VerifiedUpdatePlan 先于安装**：渠道、release、target、已签名 manifest、artifact 摘要和安装策略验证后形成模块内部不透明计划；installer 不能接受调用方拼装的 URL/version。
7. **完整性不等于真实性**：同源 SHA256 只能发现损坏；可信自更新需要验证独立签名或等价的可信 manifest，公钥/信任根随应用发布。
8. **替换失败保持旧版本**：进入 commit 前旧版本保持不变；平台无法安全原地替换时必须拒绝或使用平台专用 helper。commit 后若新版本无法启动属于独立健康检查/回滚能力，本文不做“永远可恢复”的虚假承诺。
9. **错误结构化**：CLI/TUI 根据稳定错误类别提供“重试、手动下载、权限修复或平台不支持”等差异化提示，不解析字符串。
10. **自更新不自动重启**：成功安装后返回真实路径并提示用户重启；重启决策属于交付层。

## 3. Published Language

```rust
enum UpdateChannel {
    Stable,
    Prerelease,
}

struct VersionCheckRequest {
    freshness: CheckFreshness,
}

enum CheckFreshness {
    Cached,
    ForceRefresh,
}

struct VersionCheck {
    current: AppVersion,
    latest: Option<ReleaseDescriptor>,
    source: CheckSource,
    checked_at: Timestamp,
}

enum CheckSource { Fresh, Cache, StaleCache }

struct VerifiedUpdatePlan { /* 私有构造；已验证 manifest 后才能创建 */
    source: ReleaseSourceId,
    from: AppVersion,
    target_executable: InstallPath,
    target_identity: ExecutableDigest,
    to: AppVersion,
    channel: UpdateChannel,
    target: PlatformTarget,
    artifact_url: TrustedUrl,
    artifact_sha256: Sha256Digest,
    artifact_size_limit: ByteCount,
    manifest_digest: Sha256Digest,
    trust_key_id: TrustKeyId,
    redirect_policy: RedirectPolicy,
    install_strategy: InstallStrategy,
}

enum UpdateResult {
    UpToDate { version: AppVersion },
    Installed { from: AppVersion, to: AppVersion, path: InstallPath },
}
```

`AppVersion` 在模块内部使用 semver 语义；SDK/传输层若为避免依赖 semver 使用字符串，必须由 adapter 校验后转换，不能让非法版本进入领域服务。

### 3.1 边界端口

Runtime 拥有出站 `ApplicationVersionPort`；该端口只表达 Application Control 用例：

```rust
trait ApplicationVersionPort: Send + Sync {
    async fn check(&self, freshness: CheckFreshness) -> Result<VersionCheck, UpdateError>;
    async fn apply_latest(&self) -> Result<UpdateResult, UpdateError>;
}
```

模块内部把 `apply_latest` 分解为 check/plan/apply，并使用更细的协作端口：

```rust
trait VersionCheckPort: Send + Sync {
    async fn check(
        &self,
        request: VersionCheckRequest,
    ) -> Result<VersionCheck, UpdateError>;
}

trait UpdateApplyPort: Send + Sync {
    async fn plan(&self) -> Result<VerifiedUpdatePlan, UpdateError>;

    async fn apply(
        &self,
        plan: VerifiedUpdatePlan,
    ) -> Result<UpdateResult, UpdateError>;
}
```

模块 App Service 从已注入的 UpdatePolicy 取得 channel/TTL；交付层只通过 AgentClient Application Control 表达“后台检查、强制检查、执行更新”，不传任意 channel 或 TTL。

## 4. 渠道与版本选择

- `Stable`：只接受非 prerelease 的有效 semver release；
- `Prerelease`：接受 stable 与 prerelease，按 semver precedence 选择高于当前版本的最高 release；
- draft、缺少目标平台 artifact、版本/tag 不一致或 manifest 不完整的 release 不可形成 UpdatePlan；
- downgrade 默认禁止；如未来支持必须是独立显式策略；
- channel 来自模块构造时注入的 UpdatePolicy，同一次 check/plan 固定，不能检查 stable 后下载 prerelease；
- ConfigSnapshot 中非法 channel 在 Config 边界拒绝；Config PL 经 ACL 映射为 `UpdateChannel`，不共享下游领域类型。

Release Source ACL 从供应商 DTO 提取 `ReleaseDescriptor`，保留发布 ID、版本、prerelease 标志、页面 URL、发布时间及所需资产元数据。下载 URL 必须通过允许的 HTTPS host 和 redirect policy 校验。

## 5. 检查缓存与限速

普通启动检查是非关键查询，不应阻断应用启动：

```text
check(Cached)
  ├─ fresh cache(channel) → Cache
  ├─ network success → Fresh + 更新 cache
  ├─ rate-limited/network failure + usable stale cache → StaleCache
  └─ otherwise → structured error（调用方静默降级或提示）
```

不变量：

- fresh TTL 与最大 stale age 来自 ConfigSnapshot，经 UpdatePolicy 注入；请求只选择 Cached/ForceRefresh；
- cache key 至少包含 channel 与 release source identity；
- `checked_at` 表示该 cache payload 最后一次成功从 release source 验证的时间，不是读取缓存时间；
- 超过 max stale age 的缓存不可返回；
- 相同 key 的并发检查合并 in-flight 请求，避免启动路径重复命中上游；
- `ForceRefresh` 跳过 fresh cache，但仍可在明确标注 `StaleCache` 时作为故障降级结果；
- rate-limit reset/retry-after 作为结构化元数据返回；
- cache payload 必须经过版本和 URL 校验，不缓存未验证 DTO；
缓存物理落盘通过本模块拥有的窄 `UpdateCheckCachePort`，integration adapter 负责把 cache snapshot 编码并调用 Storage `AtomicBlobPort`。cache schema/version、source identity、channel key 和 payload validator 归本模块；Storage 只保存 opaque bytes。

## 6. VerifiedUpdatePlan 与可信验证

`plan()` 在任何 artifact 下载前完成 manifest 信任验证：

1. 获取符合 channel 的 release；
2. 解析当前 `OS/ARCH` 为受支持 `PlatformTarget`；
3. 匹配唯一 artifact 与签名 manifest；
4. 下载 manifest 与签名，使用固化信任根验证发布者；
5. 从已验证 manifest 读取 artifact digest/size，并校验 release version、filename、target 一致；
6. 固化 source identity、目标 executable 的 canonical path + 当前 digest、可信 host、redirect、大小上限和安装策略；
7. 通过私有构造器生成不透明 `VerifiedUpdatePlan`。

`VerifiedUpdatePlan` 只在模块内可构造；installer adapter 接收它后仍验证 artifact bytes 的 SHA256 与大小，防止下载阶段 TOCTOU。安装时至少验证：

- HTTPS 与 host allowlist；
- redirect 次数及最终 host；
- 下载大小上限；
- manifest 的发布者签名；
- artifact SHA256 与已签名 manifest 一致；
- archive 只提取预期可执行文件，拒绝路径穿越、符号链接和额外覆盖路径；
- 解压后二进制满足平台和基本格式校验。

如果尚未建立签名发布链，模块必须把自更新标记为不可提供或显式较低信任模式；不能把同源 checksum 描述为发布者身份验证。

## 7. 安装事务

```text
Planned(verified manifest)
  → Downloading
  → ArtifactVerified
  → Staged
  → Committing
  → Installed
       └─ pre-commit failure → Unchanged
```

这是模块内部安装事务的局部生命周期，不是 Agent Run 状态机。v0.1.0 的保证边界是：进入 commit 前任何失败都保持旧 executable 不变；commit 使用平台已证明的单步 atomic replace，无法提供该原语的平台必须使用 helper 或拒绝自动更新。commit 后的启动健康检查与跨启动 rollback 不在本摘要承诺内。

### 7.1 不变量

1. stage 文件使用随机、create-new 临时路径；
2. stage 与目标处于同一文件系统，并在 commit 前 fsync；
3. commit 前不 rename/remove 当前 executable；
4. commit 是平台 adapter 的单步 atomic replace；没有该能力时禁止直接 apply；
5. pre-commit 失败清理自己的临时文件且旧版本不变；
6. commit 成功结果返回实际安装路径；
7. 使用跨进程 install lock；持锁后、commit 前重新计算 target executable canonical path 与 digest，必须匹配 VerifiedUpdatePlan，否则返回 `PlanStale` 并要求重新 plan；
8. helper 必须验证 plan digest、staged artifact digest 与目标路径，并只消费本次事务 nonce；
9. 安装过程诊断走 Logging；是否产生何种 Audit integration event 由 Audit 模块设计，二者不拥有事务状态。

## 8. 错误分类

```rust
enum UpdateErrorKind {
    Network,
    RateLimited,
    InvalidRelease,
    NoEligibleRelease,
    UnsupportedPlatform,
    ArtifactMissing,
    UntrustedSource,
    SignatureInvalid,
    IntegrityFailure,
    ArchiveInvalid,
    PermissionDenied,
    StageFailed,
    CommitFailed,
    PlanStale,
    ConcurrentUpdate,
    Configuration,
}
```

错误附带安全消息、是否可重试、rate-limit hint 和手动 release URL；不得包含完整响应 body、token、临时敏感路径或未经清洗的上游内容。

## 9. Config、Storage、Logging 与交付层边界

| 关注点 | 所有者 |
|---|---|
| Config PL 中的 channel/check_on_startup/check interval/max stale 默认值 | Config |
| Config PL → UpdateChannel/UpdatePolicy 映射 | Composition/配置 ACL |
| 渠道筛选、版本比较、TTL/stale 使用策略 | Application Version Control |
| cache 物理读写 | Storage adapter |
| release API/asset DTO 转换 | Release Source ACL |
| 下载、签名、完整性和安装事务 | Application Version Control |
| 用户确认、重启提示、状态展示 | CLI/TUI；命令经 AgentClient Application Control |
| 更新诊断日志 | Logging |
| 安全关键安装审计 | Audit |
| Git tag、release workflow、Cargo 依赖升级 | 不属于本模块 |

Composition Root 从 ConfigSnapshot 经 ACL 构造 UpdatePolicy、Release Source adapter、UpdateCheckCachePort integration adapter 与 platform installer，注入模块 App Service；AgentClient Application Control 再暴露交付层所需命令。CLI/TUI 不直接持有模块内部端口或构造 GitHub client。

## 10. 架构守卫目标

```text
Rule: update-channel-is-typed
Deny: free-form channel strings outside Config ACL/serialization

Rule: update-source-dtos-stay-in-adapter
Deny: GitHub release/asset DTO imports outside Release Source adapter

Rule: update-construction-owned-by-composition
Deny: CLI/TUI constructing concrete update gateway or HTTP client

Rule: update-apply-requires-verified-plan
Deny: installer accepting raw URL/version or publicly constructible plan without VerifiedUpdatePlan
```

## 11. 相关文档

- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Config 目标设计：[../config/README.md](../config/README.md)
- Storage 机制：[../storage/README.md](../storage/README.md)
- Logging 机制：[../logging/README.md](../logging/README.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 摘要初稿：typed channel、检查缓存、Release ACL、可信 UpdatePlan 与安装事务 | #793 |
