# Update Feature 规格

> 路径触发：`agent/features/update/**`
> 场景触发：改版本检查逻辑 / 缓存策略 / 更新渠道配置

## 架构

`update` crate 遵循 COLA 分层（`contract` → `gateway` → `api`），经 composition 装配为 `Arc<dyn UpdateService>`。

```
agent/features/update/src/
├── lib.rs        # LOG_TARGET = "aemeath:agent:update"
├── api.rs        # 公共 API re-export（跨 crate 经此模块）
├── contract.rs   # 内部数据结构（CacheEntry, GitHubRelease）+ 单元测试
└── gateway.rs    # 版本检查核心逻辑（UpdateGateway impl UpdateService）
```

## SDK 契约

`sdk::UpdateService` trait 定义在 `packages/sdk/src/update.rs`，CLI 通过 `composition::update::wire_update()` 获取 `Arc<dyn UpdateService>`。

```rust
#[async_trait]
pub trait UpdateService: Send + Sync + 'static {
    async fn check_latest(&self) -> Result<VersionCheck, SdkError>;      // 带 24h 缓存
    async fn force_check(&self) -> Result<VersionCheck, SdkError>;       // 忽略缓存
    async fn perform_update(&self) -> Result<UpdateResult, SdkError>;    // PR3 实现
}
```

> `VersionCheck` / `UpdateResult` 使用 `String` 类型（非 `semver::Version`），避免 sdk 依赖 semver。

## 版本检查策略

| 场景 | 方法 | 缓存 |
|---|---|---|
| TUI 启动 | `force_check()` | 忽略，每次查 API |
| Quiet 模式 `-q` | `check_latest()` | 24h 门控 |
| `aemeath update --check` | `force_check()` | 忽略 |
| `aemeath update` | `force_check()` → `perform_update()` | 忽略 |
| TUI `/update` | `perform_update()` | 忽略 |

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

## 缓存

- 文件路径：`~/.agents/update_check.json`
- 结构：`{ last_check, latest_version, latest_url }`
- 有效期：24 小时（`CACHE_MAX_AGE_HOURS`）
- 写入失败静默降级，不影响主流程

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
cargo test -p update                    # 12 个单元测试
cargo clippy -p update -p cli           # 零 warning
bash .agents/hooks/check-architecture-guards.sh  # 17 个 guard 通过
```
