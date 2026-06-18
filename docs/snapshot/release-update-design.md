# 发布与自动更新设计

> 对应 Issue: <https://github.com/rushsinging/aemeath/issues/307>
> 状态：**已落地**——实现见 `agent/features/update`。本文档为设计归档快照，运行规则以 `specs/update.md` 为准。

## 定位

补齐从 `git tag` → GitHub Release → 用户侧版本检查 → 交互式更新提示 → 自动下载替换的完整闭环。

三个子系统按依赖链顺序落地：

1. **GHA 自动发布**（CI/CD）——产出可下载的 release artifacts
2. **版本检查**（运行时）——检测是否有新版本
3. **自动更新**（运行时）——下载、校验、原子替换二进制

## 范围

- **平台**：仅 macOS（`aarch64-apple-darwin` + `x86_64-apple-darwin`）
- **模块归属**：新建 `agent/features/update` crate（方案 A），符合现有 COLA 架构

## 架构约束

本设计 MUST 满足以下架构守卫（参见 `docs/design/architecture-guards.md`）：

| 守卫 | 约束 | 本设计的应对 |
|---|---|---|
| #1 `check-cargo-dependency-graph.sh` | `update → {share}`；`composition` 需加入 `update`；`cli → {composition, sdk}` | 在 `business_allow` 白名单注册 `update` |
| #2 `check-cli-thin-entry.sh` | CLI 禁直连 feature crate | CLI 经 `sdk::UpdateService` trait 调用，不直引用 `update` crate |
| #5 `check-cola-layer-purity.sh` | feature 内 `contract/gateway/business/utils` 分层 | update crate 遵循 COLA 目录结构 |
| #6 `check-crate-api-boundary.sh` | 跨 feature 经 `::<crate>::api` | update 暴露 `api.rs`，只 re-export `contract` / `gateway` |
| #18 `no_mod_rs.sh` | 禁止 `mod.rs` | 使用 `<module>.rs` 而非 `mod.rs` |

### 守卫注册变更（MUST 同步）

落地时 MUST 同步修改以下文件，否则守卫阻断：

1. **`.agents/hooks/check-cargo-dependency-graph.sh`**：
   - `FEATURE_CRATES` 集合加入 `"update"`
   - `business_allow` 加入 `"update": {"share"}`
   - `business_allow["composition"]` 加入 `"update"`

2. **`.agents/hooks/check-cli-thin-entry.sh`**：
   - `FORBIDDEN_DOMAIN_CRATES` 加入 `"update"`

3. **`.agents/hooks/check-crate-api-boundary.sh`**：
   - `FEATURE_CRATES` 集合加入 `"update"`

4. **`docs/design/architecture-guards.md`**：同步更新守卫 #1、#2、#6 的白名单文档

5. **`docs/design/outline.md`**：支撑域表加入 Update 行

6. **`AGENTS.md` 触发表**：新增 `specs/update.md` 行

7. **`Cargo.toml` workspace members**：加入 `"agent/features/update"`

## 子系统 1：GHA 自动发布

### 触发

- `push tag v*`（如 `v0.8.2`）自动触发
- `workflow_dispatch`（手动触发，输入版本号）

### 版本一致性校验（job 第一步，失败即中止）

| 来源 | 提取值 | 校验规则 |
|---|---|---|
| git tag | `v0.8.2` → `0.8.2` | 去掉 `v` 前缀 |
| `Cargo.toml` | `workspace.package.version` | `0.8.2` |
| 要求 | tag 值 == Cargo.toml 值 | 不一致 → job fail |

### 发布前门禁

```yaml
- cargo fmt --check
- cargo clippy --workspace -- -D warnings
- cargo test --workspace
```

### 构建矩阵

```yaml
strategy:
  matrix:
    include:
      - target: aarch64-apple-darwin
        runner: macos-14          # Apple Silicon runner
      - target: x86_64-apple-darwin
        runner: macos-13          # Intel runner
steps:
  - cargo build --release --target ${{ matrix.target }}
```

### Artifact 命名规范

```
aemeath-{version}-{target}.tar.gz
aemeath-{version}-checksums.txt
```

示例：

```
aemeath-0.8.2-aarch64-apple-darwin.tar.gz
aemeath-0.8.2-x86_64-apple-darwin.tar.gz
aemeath-0.8.2-checksums.txt
```

tar.gz 内部结构：

```
aemeath-{version}-{target}/
└── aemeath          # 单一二进制
```

### Checksum 文件格式

```
<sha256-hex>  aemeath-{version}-aarch64-apple-darwin.tar.gz
<sha256-hex>  aemeath-{version}-x86_64-apple-darwin.tar.gz
```

### 发布步骤

1. 门禁 job 通过
2. 矩阵构建 → 每个产物打包 tar.gz
3. 汇总所有 tar.gz → 生成 checksums.txt
4. 创建 GitHub Release（tag name = release title）
5. 上传所有 artifacts + checksums.txt

### workflow 文件

`.github/workflows/release.yml`

## 子系统 2：版本检查

### API

GitHub Releases API（无需 token，匿名限速 60次/小时，远超需求）：

```
GET https://api.github.com/repos/rushsinging/aemeath/releases/latest
→ { "tag_name": "v0.9.0", "html_url": "...", "body": "release notes..." }
```

### 版本比较

使用 `semver` crate 解析并比较。从 `tag_name` 去掉 `v` 前缀后解析。

### 缓存

文件：`~/.agents/update_check.json`

```json
{
  "last_check": "2026-06-18T12:00:00Z",
  "latest_version": "0.9.0",
  "latest_url": "https://github.com/rushsinging/aemeath/releases/v0.9.0"
}
```

缓存仅用于 Quiet 模式（`-q`），距 `last_check` 超过 24 小时才发网络请求。**TUI 模式每次启动都强制查 API，不走 24h 门控。**

### 网络失败降级

- 超时 5 秒
- 网络错误 / API 限速 → 静默跳过，不报错，不影响启动
- JSON 解析失败 → 静默跳过

### 配置

`~/.agents/aemeath.json` 新增：

```json
{
  "update": {
    "check_on_startup": true,
    "channel": "stable"
  }
}
```

- `check_on_startup`：默认 `true`，用户可关闭
- `channel`：`"stable"`（仅正式 release）或 `"prerelease"`（含 pre-release tag）

### 集成方式

#### TUI 模式（默认）—— 交互式提示

```
TUI 启动
  └─ spawn tokio task（后台）
       ├─ 读配置 → check_on_startup == false → 跳过
       ├─ 调 GitHub API（每次启动都查，不走缓存门控）→ 更新缓存
       ├─ 发现新版本
       │   └─ 通过 event channel 发 UiEvent::UpdateAvailable { version, url }
       └─ TUI 收到事件 → 显示交互式 dialog
            ├─ 「现在更新」→ 执行子系统 3 的更新流程（经 Effect::RunSelfUpdate）
            ├─ 「稍后」→ 关闭 dialog
            └─ 「不再提醒」→ 写配置 check_on_startup=false → 关闭 dialog
```

TUI 交互使用现有 `AskUserBatch` 机制渲染 dialog（复用 `ConversationBlock::AskUserBatch`），不新建独立的 UI 组件。

#### Quiet 模式（`-q`）

```
main() 中同步检查（不阻塞用户输入处理）
  └─ 发现新版本 → stderr 输出一行提示
     「New version available: v0.9.0. Run `aemeath update` to update.」
```

## 子系统 3：自动更新

### 命令

```bash
aemeath update           # 检查并更新到最新版本
aemeath update --check   # 仅检查，不更新（输出当前版本、最新版本、URL）
```

### 更新流程

```
1. 调 GitHub API 获取最新版本
2. semver 比较：current >= latest?
   ├─ 是 → 输出 "Already up to date (v0.8.2)" → 退出
   └─ 否 → 继续
3. 平台匹配（std::env::consts::{OS, ARCH}）
   → 选中 aemeath-{version}-{target}.tar.gz
4. 下载 checksums.txt
5. 下载 tar.gz
6. SHA256 校验：计算下载文件的 hash，与 checksums.txt 中对应行比对
   ├─ 不匹配 → 删除下载文件 → 报错退出
   └─ 匹配 → 继续
7. 解压 tar.gz 到临时目录
8. 原子替换：
   a. current_exe() 获取当前二进制路径
   b. 在同目录写临时文件 aemeath.new
   c. fs::rename(aemeath.new, current_exe)
      ├─ macOS 同文件系统 rename 是原子的
      └─ 失败（权限）→ 提示错误
9. 输出 "Updated to v0.9.0. Please restart aemeath."
```

### 进度显示

CLI 子命令模式使用 `indicatif`（已在 CLI 依赖中）显示下载进度条。

TUI 模式下的更新进度通过 UiEvent 回灌，在状态栏显示。

### 错误处理

| 错误场景 | 处理 |
|---|---|
| 网络失败 | 清晰提示，保留原二进制 |
| checksum 不匹配 | 删除下载文件，提示完整性校验失败 |
| 权限不足 | 提示「无法写入 {path}，请检查权限」 |
| 平台不支持 | 提示当前平台无对应 artifact |
| 临时空间不足 | 清理后提示 |

### 原二进制保护

更新失败时，原二进制 **NEVER** 被修改。只有步骤 8c 的 `rename` 成功才视为更新完成。

## Crate 设计：`agent/features/update`

### COLA 分层结构

```
agent/features/update/
├── Cargo.toml
└── src/
    ├── lib.rs              ← LOG_TARGET = "aemeath:agent:update"
    ├── api.rs              ← pub use contract::*; pub use gateway::*;
    ├── contract.rs         ← 类型定义（无行为）
    └── gateway.rs          ← UpdateGateway 实现
```

> 注：`check-cola-layer-purity.sh` 要求 feature 内子目录名在 `{contract, gateway, core, business, utils}` 内。但 update crate 逻辑简单，可全部在 gateway.rs + contract.rs 中完成，无需建子目录。

### contract.rs

```rust
/// 版本检查结果。
pub struct VersionCheck {
    pub current: semver::Version,
    pub latest: semver::Version,
    pub is_update_available: bool,
    pub release_url: String,
    pub release_notes: Option<String>,
}

/// 更新配置（从 share::config 投影）。
pub struct UpdateConfig {
    pub check_on_startup: bool,
    pub channel: UpdateChannel,
}

pub enum UpdateChannel {
    Stable,
    Prerelease,
}

/// 更新执行结果。
pub enum UpdateResult {
    UpToDate(semver::Version),
    Updated { from: semver::Version, to: semver::Version },
    CheckOnly(VersionCheck),
}
```

### gateway.rs

```rust
pub struct UpdateGateway {
    http: reqwest::Client,
    cache_path: PathBuf,
}

impl UpdateGateway {
    /// 检查最新版本（Quiet 模式用，带 24h 缓存）。
    pub async fn check_latest(&self) -> Result<VersionCheck>;

    /// 强制检查（忽略缓存，用于 TUI 启动 + `aemeath update --check`）。
    pub async fn force_check(&self) -> Result<VersionCheck>;

    /// 执行更新：下载 → 校验 → 原子替换。
    pub async fn perform_update(&self) -> Result<UpdateResult>;
}
```

### api.rs

```rust
pub use crate::contract::*;
pub use crate::gateway::*;
```

## SDK trait：`UpdateService`

### packages/sdk/src/update.rs（新增）

```rust
use async_trait::async_trait;
use crate::SdkError;

#[async_trait]
pub trait UpdateService: Send + Sync + 'static {
    /// 检查最新版本（带缓存）。
    async fn check_latest(&self) -> Result<super::VersionCheck, SdkError>;

    /// 强制检查（忽略缓存）。
    async fn force_check(&self) -> Result<super::VersionCheck, SdkError>;

    /// 执行更新。
    async fn perform_update(&self) -> Result<super::UpdateResult, SdkError>;
}
```

CLI 通过 `sdk::UpdateService` trait 调用更新能力，不直接依赖 `update` crate。

### packages/sdk/src/lib.rs 变更

```rust
pub mod update;
pub use update::{UpdateResult, UpdateService, VersionCheck, UpdateChannel};
```

## Composition 装配

### agent/composition/src/lib.rs 变更

```rust
pub mod update;  // 新增
```

### agent/composition/src/update.rs（新增）

```rust
use std::sync::Arc;
use update::api::UpdateGateway;

pub type UpdateServiceHandle = Arc<dyn sdk::UpdateService>;

pub fn wire_update() -> UpdateServiceHandle {
    let gateway = UpdateGateway::new(
        reqwest::Client::new(),
        share::config::paths::global_agents_dir().join("update_check.json"),
    );
    Arc::new(gateway)
}
```

### agent/composition/src/app.rs 变更

`AgentClientBootstrap` 加入 `update: UpdateServiceHandle` 字段。

## CLI 集成

### apps/cli/src/args.rs 变更

```rust
#[derive(Subcommand)]
pub enum Commands {
    ...
    /// Check for updates and update aemeath to the latest version
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
    },
}
```

### apps/cli/src/main.rs 变更

```rust
Some(Commands::Update { check }) => {
    let update_service = composition::update::wire_update();
    subcommand::update_command::run_update_command(update_service, check).await;
}
```

### 文件结构变更

```
apps/cli/src/
├── args.rs                    ← 新增 Update 子命令
├── main.rs                    ← 路由 Update
├── subcommand/
│   ├── subcommand.rs          ← 注册 update_command 模块
│   └── update_command.rs      ← 新增：update 命令入口
└── tui/
    └── ...
    （TUI 更新提示通过 UiEvent + Effect 触发，复用 AskUserBatch dialog）
```

## 配置变更

### agent/shared/src/config/update.rs（新增）

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    pub check_on_startup: bool,
    pub channel: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            channel: "stable".to_string(),
        }
    }
}
```

### agent/shared/src/config/ 路径常量变更

`paths.rs` 新增：

```rust
pub const UPDATE_CHECK_FILE: &str = "update_check.json";

pub fn global_update_check_path() -> PathBuf {
    global_agents_dir().join(UPDATE_CHECK_FILE)
}
```

## 新增依赖

### workspace Cargo.toml `[workspace.dependencies]`

```toml
semver = "1"
sha2 = "0.10"
flate2 = "1"
tar = "0.4"
indicatif = "0.17"
```

### agent/features/update/Cargo.toml

```toml
[package]
name = "update"
version.workspace = true
edition.workspace = true

[dependencies]
share = { path = "../../shared" }
sdk = { path = "../../../packages/sdk" }
semver = { workspace = true }
sha2 = { workspace = true }
flate2 = { workspace = true }
tar = { workspace = true }
reqwest = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
log = { workspace = true }
logging = { path = "../../../packages/global/logging" }
chrono = { workspace = true }
```

### apps/cli/Cargo.toml 变更

```toml
indicatif = { workspace = true }
semver = { workspace = true }
```

> `reqwest` 需确认 `default-tls` feature（用于 HTTPS 下载）。

## TUI 集成（交互式提示）

### 新增 UiEvent

`apps/cli/src/tui/app/event.rs` 新增：

```rust
/// 版本检查发现新版本，弹出交互式更新提示。
UpdateAvailable {
    version: String,
    url: String,
},
```

### 新增 Effect

`apps/cli/src/tui/effect/effect.rs` 新增：

```rust
/// 执行自动更新（用户在 dialog 中选择「现在更新」时触发）。
RunSelfUpdate,
```

### TUI 更新检查启动时机

在 TUI `run_loop` 初始化阶段（`apps/cli/src/tui/` 中的启动函数），spawn 后台 tokio task：

```rust
// 伪代码
let update_service = bootstrap.update.clone();
tokio::spawn(async move {
    // TUI 每次启动都强制查 API，不走缓存门控
    if let Ok(check) = update_service.force_check().await {
        if check.is_update_available {
            let _ = event_tx.send(UiEvent::UpdateAvailable {
                version: check.latest.to_string(),
                url: check.release_url,
            });
        }
    }
});
```

### UiEvent::UpdateAvailable 处理

收到事件后，复用 `AskUserBatch` 机制显示 dialog：

- 问题："New version v{version} available. Update now?"
- 选项：「现在更新」/「稍后」/「不再提醒」

用户选择「现在更新」→ 发出 `Effect::RunSelfUpdate`
用户选择「不再提醒」→ 写配置 `check_on_startup=false`

### Effect::RunSelfUpdate 执行

在 Effect 执行器中调用 `update_service.perform_update()`，完成后通过 UiEvent 回灌结果。

## 验证计划

| 场景 | 验证方式 |
|---|---|
| tag 与 Cargo.toml 版本不一致 | workflow_dispatch 触发，确认 job fail |
| 跨平台 artifact 正确生成 | tag push 后检查 Release assets |
| checksums.txt 内容正确 | 手动验证 SHA256 |
| 已是最新版本 | `aemeath update --check` 输出 "up to date" |
| 有新版本 | `aemeath update --check` 输出版本信息 |
| 网络失败 | 断网测试，确认不崩溃 |
| 下载失败不破坏原二进制 | 模拟 checksum 不匹配 |
| 更新后版本正确 | `aemeath --version` 显示新版本 |
| TUI 交互式提示 | 启动 TUI 确认 dialog 显示 |
| 守卫白名单注册正确 | 守卫脚本全通过 |
| COLA 分层 | update crate 目录结构通过守卫 #5 |
| API boundary | 跨 crate 访问经 `update::api` |

## 实施顺序（建议分 3 个 PR）

### PR 1：GHA 自动发布

1. 创建 `.github/workflows/release.yml`
2. tag push 验证 artifact 生成

### PR 2：`update` crate + 版本检查 + CLI 子命令

1. 创建 `agent/features/update` crate（COLA 结构）
2. 注册守卫白名单（4 个文件）
3. 配置项（`share/config/update.rs` + `paths.rs`）
4. SDK trait（`sdk/update.rs`）
5. Composition 装配
6. CLI `update --check` 子命令
7. TUI 版本检查后台 task + UiEvent + Effect

### PR 3：自动更新（下载 + 校验 + 替换）

1. `UpdateGateway::perform_update()` 实现
2. CLI `aemeath update` 完整流程
3. TUI `Effect::RunSelfUpdate` 执行器
4. 错误处理 + 进度显示
