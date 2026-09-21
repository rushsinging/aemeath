# 非交互外部进程控制终端隔离实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Aemeath 在所有运行模式下启动的生产非交互外部进程脱离父控制终端，阻止 OrbStack/NFS 等异步终端消息污染 TUI 或调用者终端。

**Architecture:** 在 `packages/global/utils` 建立唯一 Unix 非交互进程配置能力，分别接收 `std::process::Command` 和 `tokio::process::Command`，统一在 child spawn 前执行 `setsid()`。各业务 adapter 继续拥有参数、cwd、env、stdio、协议与生命周期；Bash/Hook 继续用 child PID 作为新 session 的 PGID 完成进程树回收。架构 Guard 机械阻止生产调用点跳过该配置能力。

**Tech Stack:** Rust 2021、std/tokio process、Unix `libc::setsid`、portable-pty、Bash architecture guards、Cargo workspace。

---

## 文件结构

- Create: `packages/global/utils/src/process.rs` — 唯一非交互 child 配置能力。
- Create: `packages/global/utils/src/process_tests.rs` — Unix session、PGID 与 `/dev/tty` 契约测试。
- Modify: `packages/global/utils/src/lib.rs` — 发布窄进程配置 API，并分离现有 inline tests。
- Create: `packages/global/utils/src/lib_tests.rs` — 承接现有字符串工具测试。
- Modify: `packages/global/utils/Cargo.toml` — 增加 Unix `libc` 与 Tokio command 支持。
- Modify: 各生产调用点所属 `Cargo.toml` — 仅为尚未依赖 `utils` 的 crate 增加依赖。
- Modify: `agent/features/tools/src/adapters/{bash.rs,grep.rs,web_fetch.rs,mcp/client.rs}` — 工具进程迁移。
- Modify: `agent/features/hook/src/adapters/process.rs` — Hook 进程迁移并移除冲突的 `process_group(0)`。
- Modify: `agent/features/project/src/{adapters/git.rs,lib.rs}` — Git 进程迁移。
- Modify: `agent/features/context/src/adapters/session_legacy_workspace.rs` — legacy workspace Git 进程迁移。
- Modify: `agent/features/runtime/src/{adapters/image.rs,adapters/image/clipboard.rs,application/prompt/build/git_context.rs}` — Runtime 图片、剪贴板与 Git 进程迁移。
- Modify: `apps/cli/src/{tui/render/input/clipboard.rs,tui/effect/executor.rs}` — CLI 外部进程迁移。
- Create: `.agents/hooks/check-noninteractive-child-session.sh` — 生产调用点统一边界 Guard。
- Create: `.agents/hooks/check-noninteractive-child-session-tests.sh` — Guard 正反例测试。
- Modify: `.agents/hooks/check-architecture-guards.sh`、`.agents/architecture-guard-registry.json` — 注册并编排 Guard。
- Modify: `apps/cli/tests/pty_smoke.rs` — L5 控制终端隔离 smoke。
- Modify: `docs/design/03-engineering/{01-architecture-guards.md,04-testing-and-coverage.md}` 及受影响模块文档 — 同步 Target 与验证证据。

### Task 1: 建立失败的 Unix session 契约测试

**Files:**
- Create: `packages/global/utils/src/process_tests.rs`
- Modify: `packages/global/utils/src/lib.rs`
- Create: `packages/global/utils/src/lib_tests.rs`
- Modify: `packages/global/utils/Cargo.toml`

- [ ] **Step 1: 将 `lib.rs` 现有 inline tests 原样移动到 `lib_tests.rs`**

在 `lib.rs` 中将原 inline 模块替换为：

```rust
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
```

- [ ] **Step 2: 声明尚不存在的进程模块并写失败测试**

在 `lib.rs` 增加：

```rust
mod process;
pub use process::{configure_std_noninteractive, configure_tokio_noninteractive};
```

`process_tests.rs` 用真实 shell 输出 `pid/pgid/sid`，并尝试打开 `/dev/tty`；断言 child 的 PID、PGID、SID 相等，且 `/dev/tty` 打开失败。

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p utils process -- --nocapture`

Expected: FAIL，原因是 `process.rs` 或配置函数尚未定义。

- [ ] **Step 4: 提交测试红灯**

```bash
git add packages/global/utils
git commit -m "test(process): reproduce inherited controlling terminal"
```

### Task 2: 实现统一非交互 child 配置能力

**Files:**
- Create: `packages/global/utils/src/process.rs`
- Modify: `packages/global/utils/Cargo.toml`

- [ ] **Step 1: 实现 std command 配置**

Unix 下用 `std::os::unix::process::CommandExt::pre_exec` 安装唯一回调：

```rust
pub fn configure_std_noninteractive(command: &mut std::process::Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
```

- [ ] **Step 2: 实现 Tokio command 委托**

```rust
pub fn configure_tokio_noninteractive(command: &mut tokio::process::Command) {
    configure_std_noninteractive(command.as_std_mut());
}
```

- [ ] **Step 3: 配置依赖与平台边界**

在 `utils/Cargo.toml` 增加 Tokio；Unix target 增加 `libc`。非 Unix API 必须返回明确 unsupported 配置错误或保持编译期不可用，不得静默 no-op。

- [ ] **Step 4: 运行目标测试确认通过**

Run: `cargo test -p utils process -- --nocapture`

Expected: PASS；child 的 PID/PGID/SID 相等且 `/dev/tty` 不可用。

- [ ] **Step 5: 运行 utils 全测试与 clippy**

Run: `cargo test -p utils && cargo clippy -p utils --all-targets -- -D warnings`

Expected: PASS，0 warning。

- [ ] **Step 6: 提交基础能力**

```bash
git add packages/global/utils Cargo.lock
git commit -m "feat(process): detach noninteractive children from tty"
```

### Task 3: 迁移 Bash 与 Hook 受管进程

**Files:**
- Modify: `agent/features/tools/Cargo.toml`
- Modify: `agent/features/tools/src/adapters/bash.rs`
- Modify: `agent/features/hook/Cargo.toml`
- Modify: `agent/features/hook/src/adapters/process.rs`
- Test: `agent/features/tools/src/adapters/bash_tests.rs` 或现有对应测试文件
- Test: `agent/features/hook/src/adapters/process_tests.rs`

- [ ] **Step 1: 先扩展 Bash/Hook 测试**

为两个 adapter 增加真实 child 断言：child 回报 SID/PGID；断言均等于 child PID。保留并复用现有 timeout/cancel/后台后代回收 fixture。

- [ ] **Step 2: 运行新测试确认当前实现失败**

Run: `cargo test -p tools bash -- --nocapture && cargo test -p hook process -- --nocapture`

Expected: 至少 session-leader 断言 FAIL。

- [ ] **Step 3: 迁移命令配置**

在 stdio/cwd/env 配置完成、spawn 前调用 `configure_tokio_noninteractive`。删除 Bash 与 Hook 的 `process_group(0)`，继续用直接 child PID 作为 PGID。

- [ ] **Step 4: 运行 Tools 与 Hook 测试**

Run: `cargo test -p tools && cargo test -p hook`

Expected: PASS；timeout/cancel/残留进程组测试仍通过。

- [ ] **Step 5: 提交受管进程迁移**

```bash
git add agent/features/tools agent/features/hook Cargo.lock
git commit -m "fix(process): isolate bash and hook sessions"
```

### Task 4: 迁移其余 Tool 与 MCP stdio 进程

**Files:**
- Modify: `agent/features/tools/src/adapters/grep.rs`
- Modify: `agent/features/tools/src/adapters/web_fetch.rs`
- Modify: `agent/features/tools/src/adapters/mcp/client.rs`
- Test: 对应已有 adapter 测试文件

- [ ] **Step 1: 为 Grep/curl/MCP 启动装配增加隔离断言或配置 seam 测试**

通过可替换测试命令或 shell fixture 断言运行时 SID 等于 PID；MCP 同时验证 stdin/stdout transport 仍能完成握手或既有回环协议。

- [ ] **Step 2: 运行目标测试确认失败**

Run: `cargo test -p tools grep web_fetch mcp -- --nocapture`

Expected: 当前 child 继承 session，新增隔离断言 FAIL。

- [ ] **Step 3: 在每个 spawn/output/status 前调用统一配置函数**

不得复制 `pre_exec`/`setsid`；只导入 `utils::configure_tokio_noninteractive`。

- [ ] **Step 4: 运行 Tools 全测试**

Run: `cargo test -p tools`

Expected: PASS，包括 MCP stdio 测试。

- [ ] **Step 5: 提交 Tool/MCP 迁移**

```bash
git add agent/features/tools
git commit -m "fix(tools): detach all external tool processes"
```

### Task 5: 迁移 Project、Context 与 Runtime 外部进程

**Files:**
- Modify: `agent/features/project/Cargo.toml`
- Modify: `agent/features/project/src/adapters/git.rs`
- Modify: `agent/features/project/src/lib.rs`
- Modify: `agent/features/context/Cargo.toml`
- Modify: `agent/features/context/src/adapters/session_legacy_workspace.rs`
- Modify: `agent/features/runtime/Cargo.toml`
- Modify: `agent/features/runtime/src/adapters/image.rs`
- Modify: `agent/features/runtime/src/adapters/image/clipboard.rs`
- Modify: `agent/features/runtime/src/application/prompt/build/git_context.rs`
- Test: 各模块现有 Git/image/clipboard 测试文件

- [ ] **Step 1: 为同步与异步命令路径增加 session 隔离测试**

至少覆盖一个同步 Git adapter 与一个异步 Runtime command，断言 PID/PGID/SID；现有行为测试继续保护 cwd、输出与非零退出。

- [ ] **Step 2: 运行目标 crate 测试确认失败**

Run: `cargo test -p project && cargo test -p context session_legacy_workspace && cargo test -p runtime git_context image clipboard`

Expected: 新增 session 断言 FAIL，既有测试保持原基线。

- [ ] **Step 3: 为 crate 添加 `utils` 依赖并迁移所有命令**

同步命令使用 `configure_std_noninteractive`，Tokio 命令使用 `configure_tokio_noninteractive`；每个调用点必须在 spawn/output/status 前配置。

- [ ] **Step 4: 运行三个 crate 的完整测试**

Run: `cargo test -p project && cargo test -p context && cargo test -p runtime`

Expected: PASS。

- [ ] **Step 5: 提交业务 adapter 迁移**

```bash
git add agent/features/project agent/features/context agent/features/runtime Cargo.lock
git commit -m "fix(process): isolate project and runtime commands"
```

### Task 6: 迁移 CLI 外部进程并增加 L5 PTY smoke

**Files:**
- Modify: `apps/cli/Cargo.toml`
- Modify: `apps/cli/src/tui/render/input/clipboard.rs`
- Modify: `apps/cli/src/tui/effect/executor.rs`
- Modify: `apps/cli/tests/pty_smoke.rs`

- [ ] **Step 1: 扩展 PTY smoke**

在隔离 HOME/config 下通过 Aemeath 可控入口启动一个尝试写 `/dev/tty` 的外部 child，写入唯一 marker；断言 PTY 输出不包含 marker，同时工具结果表达 `/dev/tty` 不可用。若现有 CLI smoke 无离线工具调用入口，则增加 utils 的专用 PTY integration test，避免访问 provider。

- [ ] **Step 2: 运行 L5 测试确认失败**

Run: `cargo test -p cli --test pty_smoke -- --ignored --nocapture`

Expected: 当前实现中 marker 可进入父 PTY或 session 断言失败。

- [ ] **Step 3: 迁移 CLI 命令**

为同步/异步命令调用统一配置函数；保留 pbcopy stdin pipe、外部打开参数与 Git 查询。

- [ ] **Step 4: 运行 CLI 测试与 PTY smoke**

Run: `cargo test -p cli && cargo test -p cli --test pty_smoke -- --ignored --nocapture`

Expected: PASS；alternate screen/cursor 恢复与无控制终端断言同时成立。

- [ ] **Step 5: 提交 CLI 迁移**

```bash
git add apps/cli Cargo.lock
git commit -m "fix(cli): prevent child tty contamination"
```

### Task 7: 增加统一边界架构 Guard

**Files:**
- Create: `.agents/hooks/check-noninteractive-child-session.sh`
- Create: `.agents/hooks/check-noninteractive-child-session-tests.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 先写 Guard 正反例测试**

fixture 必须证明：直接 spawn 未配置时 exit 2；调用统一配置函数后 clean pass；测试和 build script 不误报；`utils/process.rs` 底层实现是唯一允许使用 `pre_exec/setsid` 的路径。

- [ ] **Step 2: 运行 Guard 测试确认失败**

Run: `bash .agents/hooks/check-noninteractive-child-session-tests.sh`

Expected: FAIL，因为 Guard 尚不存在或未识别违规 fixture。

- [ ] **Step 3: 实现并注册 Guard**

Guard 枚举 `apps/cli/src`、`agent/**/src`、`packages/**/src` 的生产 Rust 文件，验证所有 `Command` 启动点都经过统一命名入口。注册项使用 `target_capability_policy`，tracking issue 为 `1577`；加入 fast/full 编排。

- [ ] **Step 4: 更新 Guard 索引文档**

记录扫描范围、唯一底层例外、失败模式、正反例证据与不扫描测试/build script 的理由。

- [ ] **Step 5: 运行 Guard 单测与 fast 编排**

Run: `bash .agents/hooks/check-noninteractive-child-session-tests.sh && .agents/hooks/check-architecture-guards.sh --fast`

Expected: PASS。

- [ ] **Step 6: 提交 Guard**

```bash
git add .agents docs/design/03-engineering/01-architecture-guards.md
git commit -m "guard(process): enforce detached child sessions"
```

### Task 8: 同步模块设计与测试证据

**Files:**
- Modify: `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- Modify: `docs/design/02-modules/hook/README.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`
- Modify: 其他由实际调用点归属触发的 Target 文档

- [ ] **Step 1: 更新 Tools/Hook 进程生命周期文档**

明确“独立 session + 无控制终端 + PID=PGID=SID”，以及 timeout/cancel 仍按进程组回收。

- [ ] **Step 2: 登记 L0–L5 验证证据**

在测试架构矩阵记录 utils 契约测试、各 adapter 测试、Guard 和 PTY smoke 的命令及责任边界。

- [ ] **Step 3: 检查文档术语与代码一致**

Run: `rg -n 'process_group\(0\)|控制终端|非交互外部进程|独立 session' docs agent apps packages`

Expected: 文档不再将“独立进程组”误述为已脱离控制终端；代码和文档统一使用目标术语。

- [ ] **Step 4: 提交文档同步**

```bash
git add docs
git commit -m "docs(process): record detached session contract"
```

### Task 9: 全量验证、Issue 门禁与 PR

**Files:**
- Modify: GitHub Issue #1577 body/checklist
- Create: Pull Request to `main`

- [ ] **Step 1: 确认无裸调用与死代码**

Run: `rg -n --glob '*.rs' --glob '!**/tests/**' --glob '!**/*tests.rs' '(std::process::Command|tokio::process::Command|Command::new\()' apps agent packages`

Expected: 每个业务调用点均紧邻统一配置，且 Guard clean；无旧 `process_group(0)` 隔离逻辑。

- [ ] **Step 2: 格式化并检查 diff**

Run: `cargo fmt --all -- --check && git diff --check`

Expected: PASS。

- [ ] **Step 3: 运行相关 crate 测试**

Run: `cargo test -p utils -p tools -p hook -p project -p context -p runtime -p cli`

Expected: PASS。

- [ ] **Step 4: 运行完整架构与 workspace 门禁**

Run: `.agents/hooks/check-architecture-guards.sh --full && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS，0 warning。

- [ ] **Step 5: 运行慢速 PTY 门禁**

Run: `scripts/check-slow-test-matrix.sh`

Expected: host-native fmt/clippy/tests/P0/P1/PTY 全部 PASS；跨 target 仅按脚本默认策略执行。

- [ ] **Step 6: 更新 Issue #1577 checklist 与 Release Gate 证据**

用 `gh issue edit/comment --repo rushsinging/aemeath` 记录每项完成证据；任何不适用项必须说明理由，不得静默跳过。

- [ ] **Step 7: 同步最新 main**

Run: `git pull origin main`

Expected: 分支包含最新 `origin/main`，无未解决冲突；如有冲突，逐项保留双方测试覆盖。

- [ ] **Step 8: 复跑受同步影响的最窄门禁**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS。

- [ ] **Step 9: 推送并创建 PR**

PR 标题使用 conventional commit；body 包含 Summary、`Closes #1577`、Breaking change（严格无 TTY 的命令行为变化）和完整 Test plan。创建后查询 required checks，不执行合并。
