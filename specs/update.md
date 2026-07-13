# Update Feature 规格

> 路径触发：`agent/features/update/**`
> 场景触发：改版本检查逻辑 / 更新渠道配置

## 迁移期实现约束

`update` crate 当前使用下列扁平运行路径，并经 composition 装配为 `Arc<dyn UpdateService>`。在对应迁移 leaf 完成前，Update 改动 **MUST** 保持这些入口与现行 SDK 行为兼容。

这些文件名只描述迁移期实现，**NEVER** 作为 Update 或其他 feature 的 Target 目录模板。Target 组织 **MUST** 遵循[代码组织规范](../docs/design/01-system/06-code-organization.md)；已启用守卫的脚本行为 **MUST** 以[架构守卫注册表](../docs/design/03-engineering/architecture-guards.md)为真相源；Current → Target 差距、责任与退出条件 **MUST** 只在[迁移治理](../docs/design/03-engineering/migration-governance.md)维护。

```
agent/features/update/src/
├── lib.rs        # LOG_TARGET = "aemeath:agent:update"
├── api.rs        # 公共 API re-export（跨 crate 经此模块）
├── contract.rs   # 内部数据结构（GitHubRelease）+ 单元测试
└── gateway.rs    # 版本检查核心逻辑（UpdateGateway impl UpdateService）
```

## SDK 契约

`sdk::UpdateService` trait 定义在 `packages/sdk/src/update.rs`，CLI 通过 `composition::update::wire_update()` 获取 `Arc<dyn UpdateService>`。

```rust
#[async_trait]
pub trait UpdateService: Send + Sync + 'static {
    async fn check_latest(&self) -> Result<VersionCheck, SdkError>;      // 每次查 API（无缓存）
    async fn force_check(&self) -> Result<VersionCheck, SdkError>;       // 显式强制刷新
    async fn perform_update(&self) -> Result<UpdateResult, SdkError>;    // PR3 实现
}
```

> `VersionCheck` / `UpdateResult` 使用 `String` 类型（非 `semver::Version`），避免 sdk 依赖 semver。
>
> **无缓存策略**：`check_latest` 和 `force_check` 行为一致——每次都直接调 GitHub Releases API。
> 理由：GitHub 匿名 API 限速 60 次/小时，普通 dev tool 实际不会打满；
> 无缓存可避免过期数据漏报新版本，同时省掉 cache 文件 IO + 路径常量 + spec 章节。
> `force_check` 保留为公开方法，主要供 `aemeath update --check` 等显式场景调用，语义清晰。

## 版本检查策略

| 场景 | 方法 |
|---|---|
| TUI 启动 | `check_latest()` |
| Quiet 模式 `-q` | `check_latest()` |
| `aemeath update --check` | `force_check()` |
| `aemeath update` | `force_check()` → `perform_update()` |
| TUI `/update` | `perform_update()` |

## 自动更新流程（`perform_update`）

```
1. force_check() → 确认有新版本
2. 平台匹配（std::env::consts::{OS, ARCH}）→ 确定 artifact 文件名
3. 下载 checksums.txt → 解析对应文件的 SHA256
4. 下载 tar.gz
5. SHA256 校验 → 不匹配则报错退出
6. 解压 tar.gz → 提取 aemeath 二进制
7. 原子替换：current_exe().with_extension("new") → fs::rename
8. 提示用户重启
```

### 支持的平台

| OS | ARCH | Target Triple |
|---|---|---|
| macOS | aarch64 | `aarch64-apple-darwin` |
| macOS | x86_64 | `x86_64-apple-darwin` |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` |

### Artifact 命名

- 文件：`aemeath-{version}-{target}.tar.gz`
- 下载 URL：`https://github.com/rushsinging/aemeath/releases/download/v{version}/{filename}`
- checksums.txt 格式：`{sha256}  {filename}`（sha256sum 输出）

### 错误处理

| 错误场景 | 处理 |
|---|---|
| 网络失败 | 清晰提示，保留原二进制 |
| checksum 不匹配 | 报错，不执行替换 |
| 权限不足 | 提示「原子替换失败」 |
| 平台不支持 | 提示当前平台无对应 artifact |

## GitHub API

- Endpoint：`https://api.github.com/repos/rushsinging/aemeath/releases/latest`
- 匿名访问（无 token），限速 60 次/小时
- 超时：5 秒（`REQUEST_TIMEOUT_SECS`）
- User-Agent：`aemeath/{version}`

## 配置

`share::config::UpdateConfig`（`agent/shared/src/config/update.rs`）：

```rust
pub struct UpdateConfig {
    pub check_on_startup: bool,  // 默认 true
    pub channel: String,         // "stable" | "prerelease"，默认 "stable"
}
```

## TUI 集成

- **启动检查**：`run_loop.rs` 创建 `ui_tx` 后，调 `spawn_update_check(ui_tx.clone())`（`executor.rs`），非阻塞 spawn 后台 task
- **结果回送**：`UiEvent::UpdateAvailable { current, latest, release_url }` 经 `ui_tx` 推回
- **展示方式**：`update/ui_event.rs` 接收后调 `append_system_notice`，显示 `[aemeath v{latest} is available (you have v{current}); run \`aemeath update\` to upgrade | {release_url}]`
- **交互式升级 dialog**：PR3 中与 `perform_update` 一起实现

## 日志

- LOG_TARGET：`aemeath:agent:update`
- 日志文件路由：`~/.agents/logs/` 下无独立文件，归入兜底 `aemeath.log`（后续可新增 `agent-update.log`）

## 验证

```bash
cargo test -p update
cargo clippy -p update -p cli
bash .agents/hooks/check-architecture-guards.sh
```
