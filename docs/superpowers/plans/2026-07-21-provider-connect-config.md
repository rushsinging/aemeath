# Provider Connect 配置向导 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Issue #1457 范围内交付 Config-owned Provider Catalog、Provider UA 四级解析、`aemeath connect` pre-chat 配置向导、全局配置安全写回、Provider Probe、SDK/Composition/TUI 接线和首次聊天初始化。

**Architecture:** Config domain 拥有 Catalog、draft、状态机、校验和 UA resolver；Config application 编排 Connect 与 bootstrap；Config-owned ports 隔离 global document durable store、系统信息和 Provider Probe。Composition 只装配 façade，SDK 发布 DTO，CLI/TUI 只做命令路由和展示输入。所有跨层行为按 Domain → Application → Adapter → SDK → Consumer → 场景逐层测试。

**Tech Stack:** Rust 2021、Tokio、Serde/serde_json、async-trait、Reqwest、Clap、RatATUI/现有 TEA TUI、Cargo workspace、临时目录与测试 HTTP server。

---

## 文件地图

- `agent/shared/src/config/domain/models/types.rs`：为 `ProviderModelsConfig` 增加可选 `userAgent`，保持旧 JSON 兼容。
- `agent/features/config/src/catalog.rs`：Config-owned 静态 Catalog、ProviderSource/DriverId、推荐模型和官方 SDK UA 证据值。
- `agent/features/config/src/user_agent.rs`：UA 四级 resolver、动态系统信息格式化和 HeaderValue 校验。
- `agent/features/config/src/connect.rs`：ConnectStage、ConnectDraft、命令、View、状态转换和错误分类。
- `agent/features/config/src/ports.rs`：`GlobalConfigConnectStore`、`SystemInformationPort`、`ProviderProbePort` 契约。
- `agent/features/config/src/global_store.rs`：global document filesystem adapter、create-new、CAS、atomic durable write、bootstrap receipt rollback。
- `agent/features/config/src/application.rs`：接入 Connect application service、bootstrap coordinator 与现有 Config wiring。
- `agent/features/provider/src/adapters/client.rs`、`agent/features/provider/src/adapters/anthropic.rs`、`agent/features/provider/src/adapters/ollama.rs`、`agent/features/provider/src/adapters/openai_compatible/provider.rs`：复用正式请求构造实现最小 Probe port，删除 adapter 内 UA/base URL fallback。
- `packages/sdk/src/connect.rs`、`packages/sdk/src/client.rs`、`packages/sdk/src/lib.rs`：稳定 Connect command/view/outcome DTO 与 pre-chat façade。
- `agent/composition/src/app.rs`、`agent/composition/src/provider.rs`：pre-chat façade、Provider Probe 装配和每 Run UA 冻结。
- `apps/cli/src/args.rs`、`apps/cli/src/main.rs`、`apps/cli/src/subcommand/connect_command.rs`：注册 `connect` 子命令并路由 pre-chat façade。
- `apps/cli/src/tui/adapter/tui_runtime_event.rs`、`apps/cli/src/tui/adapter/event_mapping.rs`、`apps/cli/src/tui/app.rs`、`apps/cli/src/tui/app/update.rs`、`apps/cli/src/tui/app/scenario_tests/connect.rs`：Connect View 投影、受保护输入和类型化命令效果；不读配置/env/fs/network。
- `apps/cli/src/chat.rs` 与启动相关 Composition：首次聊天缺失 global config 的 TTY/非 TTY 判定、默认配置 receipt 和取消回滚。

---

### Task 1: 固化 ProviderModelsConfig userAgent 字段

**Files:**
- Modify: `agent/shared/src/config/domain/models/types.rs`
- Test: `agent/shared/src/config/domain/models/types_tests.rs`（在 `types.rs` 末尾用 `#[cfg(test)] #[path = "types_tests.rs"] mod tests;` 引入）

- [ ] 写失败测试：缺失 `userAgent` 反序列化为 `None`，非空值以 JSON key `userAgent` round-trip，空白值归一为 `None`。
- [ ] 运行 `cargo test -p share models::`，确认先因字段/归一化 API 不存在失败。
- [ ] 只增加 `Option<String>` 字段和 Config-owned 归一化函数，不改变已有模型解析语义。
- [ ] 运行定向测试并执行 `cargo fmt --check`。

### Task 2: 实现 Catalog 与 UA resolver

**Files:**
- Create/Modify: `agent/features/config/src/catalog.rs`
- Create/Modify: `agent/features/config/src/user_agent.rs`
- Modify: `agent/features/config/src/lib.rs`, `agent/features/config/Cargo.toml`
- Test: `agent/features/config/src/catalog_tests.rs`, `agent/features/config/src/user_agent_tests.rs`

- [ ] 先写失败测试覆盖：全部当前 driver 映射、source/driver 唯一性、Catalog 查询、推荐模型窗口合法性、官方 SDK UA 证据元数据、四级优先级、空白回退、系统版本不可用降级、HeaderValue 控制字符拒绝、UA 不泄漏密钥/路径。
- [ ] 运行 `cargo test -p config catalog user_agent`，确认测试因模块和 API 缺失失败。
- [ ] 实现最小值对象和静态 Catalog；无可靠官方 SDK UA 的条目显式使用 `None`，禁止猜测。
- [ ] 实现 `resolve_provider_user_agent(provider_config, catalog_entry, global_user_agent, system_info, version, platform)`，顺序固定为 Provider 专属 → Provider 默认 → 全局配置 → 全局默认。
- [ ] 运行 `cargo test -p config`，确认 L1 通过。

### Task 3: 实现 Connect domain 状态机

**Files:**
- Create: `agent/features/config/src/connect.rs`
- Modify: `agent/features/config/src/lib.rs`
- Test: `agent/features/config/src/connect_tests.rs`

- [ ] 先写失败测试：选择 Provider、已有 Provider 覆盖确认、拒绝覆盖返回选择页、base URL/API key/UA/model/custom model/global default/probe/review/save/cancel 可达转换；非法 stage、stale revision、重复终态和非法模型参数拒绝。
- [ ] 运行 `cargo test -p config connect`，确认先失败。
- [ ] 实现 opaque session/revision、draft、类型化命令、无密钥 View、稳定错误分类和单次业务终态不变量。
- [ ] 运行定向测试和 `cargo clippy -p config --lib --tests -- -D warnings`。

### Task 4: 实现 Config-owned ports 与 global document adapter

**Files:**
- Create: `agent/features/config/src/ports.rs`
- Create: `agent/features/config/src/global_store.rs`
- Modify: `agent/features/config/src/lib.rs`, `agent/features/config/Cargo.toml`
- Test: `agent/features/config/src/global_store_tests.rs`

- [ ] 先写失败测试：create-new 不覆盖并发创建；CAS revision 冲突不覆盖；candidate 只替换目标 Provider 并保留其他字段；atomic/durable failure 保留旧文档；receipt 仅在 identity/digest 匹配时删除。
- [ ] 运行 `cargo test -p config global_store`，确认先失败。
- [ ] 使用现有 storage atomic API；新增 Config-owned store port 和 filesystem adapter，跨进程锁使用运行时根目录下稳定 lock 文件，禁止 CLI/TUI 直接访问 fs。
- [ ] 运行 `cargo test -p config`，并用每测试唯一 tempfile 验证磁盘内容和 receipt 行为。

### Task 5: 接入 Connect application service

**Files:**
- Modify: `agent/features/config/src/application.rs`, `agent/features/config/src/contract.rs`, `agent/features/config/src/lib.rs`
- Test: `agent/features/config/tests/connect_application.rs`

- [ ] 先写失败 L2 测试：StartConnect 返回 Catalog View；Apply command 只推进服务端状态；Probe 成功不保存；Probe 失败必须显式继续；CAS 冲突返回 typed failure；主动取消逐字节保持原文不变。
- [ ] 运行 `cargo test -p config --test connect_application`，确认先失败。
- [ ] 将 Connect façade 注入现有 Config wiring，保持 `ConfigWriter` 只写 project override；global Connect 使用独立 store。
- [ ] 运行 Config 全部测试和架构 guard，确认没有 TUI/CLI/config 路径旁路。

### Task 6: 实现 Provider Probe 与请求复用

**Files:**
- Modify: `agent/features/provider/src/adapters/client.rs`, `agent/features/provider/src/adapters/anthropic.rs`, `agent/features/provider/src/adapters/ollama.rs`, `agent/features/provider/src/adapters/openai_compatible/provider.rs`
- Modify: `agent/composition/src/provider.rs`
- Test: `agent/features/provider/tests/probe_contract.rs`、现有 adapter request tests

- [ ] 先写失败 L2/L3 测试：Probe 使用最小有效 LLM 请求；正式请求和 Probe 的 endpoint、认证规则、最终 UA 相同；Probe 不读 env、不写 Config、不透明重试；错误映射稳定且敏感字段不出现在错误/日志。
- [ ] 运行 `cargo test -p provider probe_contract`，确认先失败。
- [ ] 让各支持 driver 通过统一 request builder 实现 Probe port；adapter 只接受最终解析 UA 与已校验 endpoint，不复制 Catalog 默认值。
- [ ] 运行 `cargo test -p provider` 与 `cargo test -p composition provider`。

### Task 7: 发布 SDK DTO 并接通 Composition pre-chat façade

**Files:**
- Create: `packages/sdk/src/connect.rs`
- Modify: `packages/sdk/src/lib.rs`, `packages/sdk/src/client.rs`, `packages/sdk/src/commands.rs`
- Modify: `agent/composition/src/app.rs`, `agent/composition/src/provider.rs`, `agent/composition/src/runtime.rs`
- Test: `packages/sdk/tests/connect_contract.rs`, `agent/composition/tests/connect_wiring.rs`

- [ ] 先写失败 L3 契约测试：Connect command 携带 session id/revision；View 不含 committed API key；stage/error/outcome 字段完整且可序列化；ExplicitCommand 与 FirstChatBootstrap origin 可区分。
- [ ] 运行 `cargo test -p sdk --test connect_contract`，确认先失败。
- [ ] 实现最小 DTO/ACL 映射和 pre-chat façade；不要向 SDK 暴露 ConfigReader、ConfigWriter、store 或路径。
- [ ] 在 Run admission 处调用 Config-owned UA resolver 一次并注入 invocation scope，验证同一 Run 内 reload 不改变 UA。
- [ ] 运行 SDK/Composition 契约测试和 clippy。

### Task 8: 注册 CLI connect 并实现 TUI 投影

**Files:**
- Modify: `apps/cli/src/args.rs`, `apps/cli/src/main.rs`, `apps/cli/src/subcommand.rs`
- Create: `apps/cli/src/subcommand/connect_command.rs`
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`, `apps/cli/src/tui/adapter/event_mapping.rs`, `apps/cli/src/tui/app.rs`, `apps/cli/src/tui/app/update.rs`, `apps/cli/src/tui/render.rs`
- Test: `apps/cli/src/command_contract_tests.rs`, `apps/cli/src/tui/adapter/event_mapping_tests.rs`, `apps/cli/src/tui/app/scenario_tests/connect.rs`

- [ ] 先写失败测试：`aemeath connect` 能解析且不启动 MainSession；TUI 显示 Catalog/stage/errors；API key 仅遮罩；Effect 发送 typed command；stale/probe/persist/rollback DTO 直接展示。
- [ ] 运行 `cargo test -p cli command_contract` 与 Connect 场景定向测试，确认先失败。
- [ ] 实现 CLI 路由和 TUI reducer/view/effect，禁止在 UI 层解析 URL、模型限制、UA 或访问 config/env/fs/network。
- [ ] 运行 CLI 测试、TUI 场景测试和相关架构 guards。

### Task 9: 实现首次聊天初始化

**Files:**
- Modify: `apps/cli/src/chat.rs`, `agent/composition/src/app.rs`, `agent/features/config/src/application.rs`
- Test: `agent/features/config/tests/bootstrap_scenarios.rs`、CLI L4/L5 smoke

- [ ] 先写失败测试：global 不存在 + 非 TTY 返回 `InteractiveSetupRequired` 且不创建文件；TTY create 完整 `Config::default()`；Connect 成功后继续聊天；取消只删除匹配 receipt；外部修改后 rollback refusal；help/version/其他子命令不创建文件。
- [ ] 运行定向场景测试，确认先失败。
- [ ] 实现 create-new、receipt 传递、Connect origin 和聊天继续/取消分支；不得让主动 Connect 的取消修改原配置。
- [ ] 运行 Config/Composition/CLI 场景测试。

### Task 10: 完整验证与 Issue 门禁

**Files:**
- Modify: 必要时同步 `docs/design/02-modules/**`、`specs/3.3-tui-cli.md`、`specs/3.6-provider.md`、`specs/3.9-config-compat.md`
- Verify: Issue #1457 checklist、architecture guards、workspace checks

- [ ] 运行 `cargo fmt --all -- --check`。
- [ ] 运行所有受影响 crate 的 `cargo test -p config -p provider -p sdk -p composition -p cli`。
- [ ] 运行 `cargo check --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings`。
- [ ] 运行仓库架构 guards、`git diff --check`、生产可达性检查和链接检查。
- [ ] 逐项更新 Issue acceptance/checklist 对齐状态；未完成项记录原因、影响和后续，不宣称完成。
- [ ] 复核无重复 Provider 默认值、无旧 UA 旁路、无死代码和无密钥泄漏。
