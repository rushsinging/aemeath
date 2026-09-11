# 通用 Config Form TUI 与 Connect 迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `aemeath connect` 从 raw-mode 手工输出迁移到 Config-owned Schema-driven 通用 Form 协议与 Ratatui 宿主，修复布局错乱，并为后续配置设置提供可复用 TUI 能力。

**Architecture:** Config BC 定义 workflow、schema、session、revision、业务校验和 Connect ACL；SDK 原样发布通用 Form Published Language；CLI/TUI 只维护通用 model/reducer/effect 与 Ratatui renderer。Connect 和首次聊天初始化共用同一 `ConfigFormClient` 与 Form 宿主，Provider、持久化和状态机逻辑不进入 TUI。

**Tech Stack:** Rust 2021、Tokio、Serde、`async-trait`、Ratatui 0.30、Crossterm 0.28、TestBackend、Cargo test/clippy、architecture guards。

---

## 文件边界

### Config BC

- 创建 `agent/features/config/src/core/form/mod.rs`：通用 Form 领域值对象、workflow/session/revision、schema、view、command、typed value、error、busy 和 terminal。
- 创建 `agent/features/config/src/core/form/service.rs`：Config-owned Form session facade，将通用命令路由到 workflow handler。
- 创建 `agent/features/config/src/core/form/service_tests.rs`：workflow 注册、revision、原子页面提交、错误和 Secret 回流测试。
- 创建 `agent/features/config/src/core/form/provider_connect.rs`：`provider_connect` workflow 的 ACL，负责 Connect 领域 View/Command 与通用 Form View/Command 转换。
- 创建 `agent/features/config/src/core/form/provider_connect_tests.rs`：每个 Connect stage 的 schema、action、typed value 与领域命令契约测试。
- 修改 `agent/features/config/src/connect.rs`：注册通用 Form 模块，并保留 Connect 领域服务作为 ACL 的内部目标。

### SDK Published Language

- 创建 `packages/sdk/src/config_form.rs`：`ConfigFormClient`、通用 DTO、typed command/value、error 和 terminal outcome。
- 创建 `packages/sdk/tests/config_form_contract.rs`：Serde/schema、Secret 不回流、revision/workflow/session 字段完整性测试。
- 修改 `packages/sdk/src/lib.rs`：导出通用 Form Published Language，删除被一次性替换的旧 Connect UI 协议导出。
- 修改 `packages/sdk/src/client.rs`：提供 `ConfigFormClient` trait；不保留旧 Connect client 双协议兼容层。

### Composition

- 修改 `agent/composition/src/app.rs`：把 Config Form facade 适配到 SDK，保留 `ConnectFacade` 的 Provider Probe、durable store 和 bootstrap 装配边界；TUI 不直接依赖 Config 类型。
- 修改 `agent/composition/tests/connect_wiring.rs`：验证 Form workflow、origin、revision、字段完整性和 Secret redaction 的 Composition 契约。

### CLI/TUI

- 创建 `apps/cli/src/subcommand/config_form_command.rs`：通用 Form model、reducer、effect、宿主事件循环和终态返回。
- 创建 `apps/cli/src/subcommand/config_form_render.rs`：通用 Form view assembler 与 Ratatui renderer，使用现有主题语义色。
- 创建 `apps/cli/src/subcommand/config_form_tests.rs`：model/reducer/Secret/typed value 测试。
- 创建 `apps/cli/src/subcommand/config_form_scenario_tests.rs`：Ratatui `TestBackend` 宽屏、窄屏、resize、页面旅程和旧帧清理场景。
- 修改 `apps/cli/src/subcommand.rs`：注册通用 Form 模块。
- 修改 `apps/cli/src/subcommand/connect_command.rs`：删除旧 Connect reducer、文本 parser、`ConnectProjection`、raw-mode renderer；只启动 `provider_connect` 并委托通用宿主。
- 修改 `apps/cli/src/main.rs`：Connect 路由使用通用 Form bootstrap，不再暴露旧 Connect UI 类型。
- 修改 `apps/cli/src/command_contract_tests.rs`：将旧 Connect UI 断言迁移为通用 Form model/effect 契约。

### Guard 与文档

- 创建 `.agents/hooks/check-projection-naming.sh`：扫描 Rust 标识符中的 `Projection` / `projection`，跳过注释、字符串和外部稳定数据。
- 创建 `.agents/hooks/tests/check-projection-naming.sh`：故意违规、注释/字符串无误报、恢复后通过。
- 修改 `.agents/hooks/check-architecture-guards.sh` 或唯一 Guard 编排入口：注册新命名 Guard。
- 修改 `docs/design/03-engineering/01-architecture-guards.md`：登记 Guard、扫描边界和测试命令。
- 修改 `specs/3.3-tui-cli.md`：将 Connect 展示边界更新为通用 Form Published Language。
- 修改 `specs/3.9-config-compat.md`：补充 Config Form workflow/session 的 Config BC 所有权。
- 修改 `docs/superpowers/specs/2026-08-01-config-form-tui-connect-design.md`：同步实现差异、保留范围和实际类型名。

---

### Task 1: 同步基线并建立实施任务

**Files:**
- Modify: `docs/superpowers/plans/2026-08-01-config-form-tui-connect-plan.md`

- [ ] **Step 1: 拉取最新主分支并确认分支基线**

Run:

```bash
git fetch origin main
git merge origin/main
```

Expected: 合并成功；若存在冲突，只解决当前 PR 分支与 `origin/main` 的实际冲突，不重新引入已退役的旧 Runtime 路径。

- [ ] **Step 2: 验证基线编译与现有 Connect 测试**

Run:

```bash
cargo test -p config -p sdk -p composition -p cli
```

Expected: 记录基线失败；新增失败必须能区分为既有问题或本次同步冲突。

- [ ] **Step 3: 提交基线同步结果**

```bash
git add -A
git commit -m "chore(connect): 同步主分支实施基线"
```

---

### Task 2: 建立通用 Form Domain 类型

**Files:**
- Create: `agent/features/config/src/core/form/mod.rs`
- Create: `agent/features/config/src/core/form/service.rs`
- Create: `agent/features/config/src/core/form/service_tests.rs`
- Modify: `agent/features/config/src/connect.rs`

- [ ] **Step 1: 写 workflow、schema 和 typed value 的失败测试**

测试必须先表达这些行为：

```rust
#[test]
fn provider_connect_workflow_publishes_stable_page_and_field_ids() {
    let view = provider_connect_registry().start(ConnectFormOrigin::ExplicitCommand).unwrap();
    assert_eq!(view.workflow_id.as_str(), "provider_connect");
    assert_eq!(view.page.id.as_str(), "select_provider");
    assert!(view.page.fields.iter().any(|field| field.id.as_str() == "provider_source"));
}

#[test]
fn page_submission_rejects_stale_revision_without_mutating_session() {
    let mut service = test_form_service();
    let initial = service.start("provider_connect", FormOrigin::ExplicitCommand).unwrap();
    let error = service.submit(initial.session_id.clone(), FormRevision::initial(), vec![]).unwrap_err();
    assert!(matches!(error, FormError::StaleRevision { .. }));
    assert_eq!(service.view(initial.session_id).unwrap().revision, initial.revision);
}

#[test]
fn secret_values_never_appear_in_published_view() {
    let view = view_with_secret_field_value("secret-key");
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(!serialized.contains("secret-key"));
    assert!(serialized.contains("has_value"));
}
```

- [ ] **Step 2: 运行失败测试确认失败原因**

Run:

```bash
cargo test -p config provider_connect_workflow_publishes_stable_page_and_field_ids page_submission_rejects_stale_revision_without_mutating_session secret_values_never_appear_in_published_view
```

Expected: FAIL，因为通用 Form domain 类型和 service 尚未存在；不得通过修改断言消除失败。

- [ ] **Step 3: 实现最小通用 Domain**

实现以下稳定边界：

```rust
pub struct ConfigFormWorkflowId(String);
pub struct ConfigFormSessionId(String);
pub struct ConfigFormRevision(u64);
pub enum ConfigFormOrigin { ExplicitCommand, FirstChatBootstrap }
pub enum ConfigFormFieldType { Text, Secret, Number, SingleSelect, Boolean, Summary, Status }
pub enum ConfigFormValue { Text(String), Secret(String), Number(u64), Boolean(bool), SelectedOption(String) }
pub struct ConfigFormView { pub workflow_id: ConfigFormWorkflowId, pub session_id: ConfigFormSessionId, pub revision: ConfigFormRevision, pub page: ConfigFormPage, pub busy: Option<ConfigFormBusy>, pub terminal: Option<ConfigFormTerminal> }
pub enum ConfigFormCommand { SubmitPage { values: Vec<ConfigFormFieldValue> }, InvokeAction { action_id: String }, Back, Cancel, Refresh }
pub trait ConfigFormWorkflow: Send + Sync { ... }
```

要求：revision 在每个成功状态变更后递增；页面提交失败不得变更 revision 或草稿；Secret 在 View 中只发布 `has_value`；workflow ID 与 field/action ID 由服务端发布。

- [ ] **Step 4: 运行 Config domain 测试**

```bash
cargo test -p config core::form
```

Expected: PASS，且没有 Secret 明文、revision 覆写或任意 JSON 值旁路。

- [ ] **Step 5: 提交 Domain**

```bash
git add agent/features/config/src/core/form agent/features/config/src/connect.rs
git commit -m "feat(config): 建立通用 Config Form domain"
```

---

### Task 3: 将 Connect 状态机接入 `provider_connect` Form ACL

**Files:**
- Create: `agent/features/config/src/core/form/provider_connect.rs`
- Create: `agent/features/config/src/core/form/provider_connect_tests.rs`
- Modify: `agent/features/config/src/core/form/mod.rs`
- Modify: `agent/features/config/src/core/connect/view.rs`
- Modify: `agent/features/config/src/core/connect/service.rs`

- [ ] **Step 1: 写每个 Connect stage 的 ACL 失败测试**

覆盖 `SelectProvider`、`ConfirmOverwrite`、`EditEndpoint`、`EditCredential`、`EditUserAgent`、`SelectModel`、`EditCustomModel`、`ChooseGlobalDefault`、`ChooseProbe`、`Probing`、`Review`、`Saving`、`Completed` 和 `Cancelled`。每个测试至少断言 page ID、field ID、action ID 和传给 Connect service 的 typed command。

```rust
#[test]
fn select_provider_page_uses_option_id_and_maps_to_catalog_command() {
    let mut adapter = provider_connect_adapter_with_catalog();
    let view = adapter.start(FormOrigin::ExplicitCommand).unwrap();
    let provider = view.page.fields.iter().find(|field| field.id.as_str() == "provider_source").unwrap();
    assert!(provider.options.iter().any(|option| option.id == "anthropic"));
    let next = adapter.submit(view, vec![selected("provider_source", "anthropic")]).unwrap();
    assert_eq!(next.page.id.as_str(), "edit_endpoint");
}

#[test]
fn credential_page_accepts_secret_and_review_view_only_exposes_presence() {
    let view = adapter_at("edit_credential");
    let next = adapter.submit(view, vec![secret("api_key", "secret-key")]).unwrap();
    let json = serde_json::to_string(&next).unwrap();
    assert!(!json.contains("secret-key"));
    assert!(next.page.summary.iter().any(|item| item.value == "已设置"));
}
```

- [ ] **Step 2: 运行失败测试**

```bash
cargo test -p config provider_connect_page
```

Expected: FAIL，因为 ACL 尚未提供通用 page schema 和 command mapping。

- [ ] **Step 3: 实现 ACL**

`provider_connect.rs` 负责：

- 使用 Config Catalog 生成 provider option、默认 endpoint 和推荐模型；
- 将 Connect `ConnectView` 转为通用 Form view；
- 将 field values/action ID 转为 Connect typed command；
- 将 `ConnectError` 映射为 field/page typed error；
- 将 `Probing`/`Saving` 映射为 Busy 与 refresh policy；
- 将 Completed/Cancelled 映射为通用 terminal；
- 保持 Connect service 的覆盖、Probe、CAS、bootstrap receipt 语义不变。

TUI 不再通过 stage 解析命令；所有动作使用由 ACL 发布的 action ID。

- [ ] **Step 4: 运行 Config 层完整测试**

```bash
cargo test -p config
```

Expected: PASS；原 Connect 状态机测试与新增 Form ACL 测试同时通过。

- [ ] **Step 5: 提交 ACL**

```bash
git add agent/features/config/src/core/form agent/features/config/src/core/connect
 git commit -m "feat(config): 通过通用 Form 发布 Connect workflow"
```

---

### Task 4: 发布 SDK Config Form Protocol

**Files:**
- Create: `packages/sdk/src/config_form.rs`
- Create: `packages/sdk/tests/config_form_contract.rs`
- Modify: `packages/sdk/src/client.rs`
- Modify: `packages/sdk/src/lib.rs`

- [ ] **Step 1: 写 SDK 契约失败测试**

```rust
#[test]
fn config_form_wire_schema_preserves_workflow_session_revision_and_typed_values() {
    let command = sdk::ConfigFormCommand::SubmitPage {
        session_id: sdk::ConfigFormSessionId("session-1".into()),
        expected_revision: sdk::ConfigFormRevision(4),
        values: vec![sdk::ConfigFormFieldValue::selected("provider_source", "anthropic")],
    };
    let json = serde_json::to_value(command).unwrap();
    assert_eq!(json["session_id"], "session-1");
    assert_eq!(json["expected_revision"], 4);
    assert_eq!(json["values"][0]["value_type"], "selected_option");
}

#[test]
fn config_form_view_never_serializes_secret_text() {
    let view = secret_redacted_view();
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("secret-key"));
    assert!(json.contains("has_value"));
}
```

- [ ] **Step 2: 运行失败测试**

```bash
cargo test -p sdk config_form
```

Expected: FAIL，因为 SDK trait/DTO 尚未发布。

- [ ] **Step 3: 实现 SDK Published Language**

发布 `ConfigFormClient`：

```rust
#[async_trait]
pub trait ConfigFormClient: Send + Sync {
    async fn start_form(&self, workflow_id: ConfigFormWorkflowId, origin: ConfigFormOrigin) -> Result<ConfigFormView, SdkError>;
    async fn submit_page(&self, command: ConfigFormSubmitPage) -> Result<ConfigFormView, SdkError>;
    async fn invoke_action(&self, command: ConfigFormInvokeAction) -> Result<ConfigFormView, SdkError>;
    async fn cancel_form(&self, session_id: ConfigFormSessionId, revision: ConfigFormRevision) -> Result<ConfigFormView, SdkError>;
    async fn refresh_form(&self, session_id: ConfigFormSessionId) -> Result<Option<ConfigFormView>, SdkError>;
}
```

DTO 必须包含稳定 ID、typed value、field/page error、busy refresh policy 和 terminal outcome。不得导出 `ConnectStage`、`ConnectDraftView` 或 `ConnectAvailableAction` 作为 UI 协议。

- [ ] **Step 4: 运行 SDK 契约测试**

```bash
cargo test -p sdk
```

Expected: PASS，且 JSON schema 不丢 workflow/session/revision/field/action 信息。

- [ ] **Step 5: 提交 SDK 协议**

```bash
git add packages/sdk/src packages/sdk/tests
git commit -m "feat(sdk): 发布通用 Config Form 协议"
```

---

### Task 5: 修改 Composition Adapter 与 bootstrap

**Files:**
- Modify: `agent/composition/src/app.rs`
- Modify: `agent/composition/tests/connect_wiring.rs`

- [ ] **Step 1: 写 Composition Form client 失败契约**

```rust
#[tokio::test]
async fn connect_bootstrap_exposes_provider_connect_as_generic_form_workflow() {
    let bootstrap = build_connect_bootstrap_with_agents_dir(temp.path()).await.unwrap();
    let view = bootstrap.forms.start_form(
        sdk::ConfigFormWorkflowId("provider_connect".into()),
        sdk::ConfigFormOrigin::ExplicitCommand,
    ).await.unwrap();
    assert_eq!(view.workflow_id.as_str(), "provider_connect");
    assert!(view.page.fields.iter().any(|field| field.id.as_str() == "provider_source"));
}
```

- [ ] **Step 2: 运行失败测试**

```bash
cargo test -p composition connect_bootstrap_exposes_provider_connect_as_generic_form_workflow
```

Expected: FAIL，因为 bootstrap 当前只发布旧 `ConnectClient`。

- [ ] **Step 3: 实现 Composition ACL facade**

让 `ConnectFacade` 或职责更窄的 `ConfigFormFacade` 实现 SDK `ConfigFormClient`：

- workflow ID 只允许 `provider_connect`；
- session/revision 原样传递；
- typed values/action ID 原样传给 Config ACL；
- Provider Probe、Global store、bootstrap receipt 仍由 Composition 装配；
- 不在 Composition 重建 Catalog、默认值或业务校验。

`ConnectBootstrap` 发布 `Arc<dyn sdk::ConfigFormClient>`；旧 Connect client 只保留到所有消费者迁移完成的同一提交内，不建立兼容分支。

- [ ] **Step 4: 运行 Composition 契约与 wiring 测试**

```bash
cargo test -p composition
```

Expected: PASS，覆盖主动 connect、首次聊天 origin、非 TTY 初始化和无 MainSession 装配。

- [ ] **Step 5: 提交 Composition**

```bash
git add agent/composition/src/app.rs agent/composition/tests/connect_wiring.rs
git commit -m "feat(composition): 装配通用 Config Form client"
```

---

### Task 6: 实现通用 Form TUI model/reducer/effect

**Files:**
- Create: `apps/cli/src/subcommand/config_form_command.rs`
- Create: `apps/cli/src/subcommand/config_form_tests.rs`
- Modify: `apps/cli/src/subcommand.rs`

- [ ] **Step 1: 写 TUI reducer 失败测试**

```rust
#[test]
fn secret_input_emits_typed_secret_and_masks_render_value() {
    let mut model = ConfigFormModel::new(secret_field_view());
    model.update(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(model.visible_input(), "•");
    let effect = model.update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    assert!(matches!(effect, ConfigFormEffect::SubmitPage { values, .. }
        if values == vec![ConfigFormFieldValue::secret("api_key", "s")]
    ));
}

#[test]
fn server_view_replacement_clears_secret_and_keeps_revision_from_server() {
    let mut model = ConfigFormModel::new(secret_field_view());
    model.replace_view(next_server_view());
    assert_eq!(model.visible_input(), "");
    assert_eq!(model.view().revision, sdk::ConfigFormRevision(2));
}
```

- [ ] **Step 2: 运行失败测试**

```bash
cargo test -p cli config_form
```

Expected: FAIL，因为通用 model/reducer/effect 尚不存在。

- [ ] **Step 3: 实现纯 reducer**

实现 `ConfigFormModel`、`ConfigFormEffect` 和 `ConfigFormInputMode`：

- 仅维护当前 View、本地非敏感输入、焦点和滚动；
- Enter 产生 typed SubmitPage/InvokeAction；
- Esc 产生带 session/revision 的 Cancel；
- Busy 只按 View refresh policy 产生 Refresh；
- 不匹配 field/action ID 时不猜测业务；
- View replacement 原子替换服务端状态并清空 Secret；
- 不直接调用网络、文件、env 或 tokio spawn。

- [ ] **Step 4: 运行 CLI 单元测试**

```bash
cargo test -p cli config_form
```

Expected: PASS。

- [ ] **Step 5: 提交 TUI model**

```bash
git add apps/cli/src/subcommand/config_form_command.rs apps/cli/src/subcommand.rs apps/cli/src/subcommand/config_form_tests.rs
git commit -m "feat(cli): 增加通用 Config Form reducer"
```

---

### Task 7: 实现 Ratatui renderer 与布局场景测试

**Files:**
- Create: `apps/cli/src/subcommand/config_form_render.rs`
- Create: `apps/cli/src/subcommand/config_form_scenario_tests.rs`
- Modify: `apps/cli/src/subcommand/config_form_command.rs`

- [ ] **Step 1: 写 TestBackend 失败场景**

场景必须先验证：

```rust
#[test]
fn provider_form_wide_screen_uses_two_columns_without_row_drift() { ... }

#[test]
fn provider_form_narrow_screen_falls_back_to_single_column() { ... }

#[test]
fn replacing_page_clears_previous_frame_content() { ... }

#[test]
fn resizing_form_recomputes_layout_without_stale_lines() { ... }

#[test]
fn secret_and_long_endpoint_never_leak_or_shift_footer() { ... }
```

语义断言要求：每个已绘制逻辑行的第一个非空单元位于预期列；底部操作区始终位于最后三行；旧页面标题和字段不得存在于替换后的 buffer；Secret 明文不得存在于 buffer。

- [ ] **Step 2: 运行失败场景测试**

```bash
cargo test -p cli config_form_scenario
```

Expected: FAIL，因为 renderer 和场景 harness 尚未存在。

- [ ] **Step 3: 实现 renderer**

使用 Ratatui `Frame`、`Layout`、`Block`、`Paragraph`、`List` 和语义色常量：

- 宽度达到双栏阈值时左字段/选项、右说明/摘要；
- 窄屏单栏；
- 高度不足时中区滚动，底部动作固定；
- 每帧只从当前 View 绘制，不执行 ANSI 清屏或 `print!`；
- Secret 使用遮罩文本，summary 只显示 Config 发布的 redacted value；
- 长 URL、模型 ID 和错误文本使用 `Wrap`/截断，不破坏 footer。

- [ ] **Step 4: 运行 Ratatui 场景测试**

```bash
cargo test -p cli config_form_scenario
```

Expected: PASS；宽屏、窄屏、resize、页面替换、长字段和 Secret 场景均通过。

- [ ] **Step 5: 提交 renderer 与场景**

```bash
git add apps/cli/src/subcommand/config_form_render.rs apps/cli/src/subcommand/config_form_scenario_tests.rs apps/cli/src/subcommand/config_form_command.rs
git commit -m "feat(cli): 使用 Ratatui 渲染通用 Config Form"
```

---

### Task 8: 迁移 Connect 命令并删除旧 UI 协议

**Files:**
- Modify: `apps/cli/src/subcommand/connect_command.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/command_contract_tests.rs`
- Modify: `apps/cli/src/subcommand.rs`
- Modify: `packages/sdk/src/connect.rs`
- Modify: `packages/sdk/src/lib.rs`

- [ ] **Step 1: 写 Connect 路由失败场景**

```rust
#[tokio::test]
async fn explicit_connect_runs_generic_provider_workflow_and_returns_terminal_outcome() { ... }

#[test]
fn connect_command_contains_no_stage_parser_or_manual_frame_renderer() {
    let source = std::fs::read_to_string("src/subcommand/connect_command.rs").unwrap();
    assert!(!source.contains("ConnectProjection"));
    assert!(!source.contains("println!");
    assert!(!source.contains("ConnectStage"));
}
```

- [ ] **Step 2: 运行失败测试**

```bash
cargo test -p cli explicit_connect_runs_generic_provider_workflow_and_returns_terminal_outcome connect_command_contains_no_stage_parser_or_manual_frame_renderer
```

Expected: 第二项必然 FAIL，证明旧 UI 路径仍在生产代码中。

- [ ] **Step 3: 替换 Connect 路由**

`run_connect_command_with_origin` 只做：

1. 检查 stdin/stdout TTY；
2. 从 Composition 获取 `ConfigFormClient`；
3. 以 `provider_connect` 和 origin 启动通用 Form；
4. 进入共享 Ratatui 全屏宿主；
5. 按通用 Effect 调用 SDK client；
6. 返回通用 terminal outcome。

删除 `ConnectUiModel`、`ConnectEffect`、`ConnectInputMode`、`ConnectProjection`、`command_for_input`、所有 stage-specific parser、手写 `render` 和 Connect 专用 raw-mode guard。删除 SDK 旧 Connect UI DTO/trait；Config 领域类型只能留在 Config BC 和 ACL 内部。

- [ ] **Step 4: 运行 Connect 场景和 CLI 测试**

```bash
cargo test -p cli -p sdk -p composition
```

Expected: PASS；旧 UI 契约不存在，新的 Form workflow 旅程通过。

- [ ] **Step 5: 提交 Connect 迁移**

```bash
git add apps/cli/src packages/sdk/src packages/sdk/tests agent/composition/src agent/composition/tests
git commit -m "refactor(connect): 迁移到通用 Config Form TUI"
```

---

### Task 9: 补齐全链路场景与首次启动回滚验证

**Files:**
- Create or modify: `apps/cli/tests/config_form_connect_scenarios.rs`
- Modify: `agent/composition/tests/connect_wiring.rs`
- Modify: `agent/features/config/src/core/connect/service_tests.rs`

- [ ] **Step 1: 写跨层失败场景**

覆盖：

- Provider 选择 → endpoint → credential → model → review → save；
- 同名 Provider 覆盖确认；
- 拒绝覆盖返回 provider 选择页；
- Probe 失败后只有显式 Continue 才能保存；
- 主动 connect 取消无配置变更；
- 首次聊天成功继续聊天；
- 首次聊天取消按 receipt 删除本次默认配置；
- 非 TTY 不创建文件；
- connect bootstrap 不创建 MainSession。

每层都断言下一层收到的字段和 typed command，不只断言最终 terminal。

- [ ] **Step 2: 运行失败场景**

```bash
cargo test --test config_form_connect_scenarios -p cli
cargo test -p composition connect
cargo test -p config connect
```

Expected: 至少覆盖尚未迁移的旧 SDK/CLI 边界并失败，失败原因应指向缺少 generic Form 链路或旧协议引用。

- [ ] **Step 3: 实现并修复组合链路**

只修复缺失的 Config → Composition → SDK → CLI/TUI 边界；不得把业务判断下沉到 TUI，不得通过复制 Config Catalog/default/Probe 逻辑止血。

- [ ] **Step 4: 运行全部跨层场景**

```bash
cargo test --test config_form_connect_scenarios -p cli
cargo test -p config -p composition -p sdk -p cli
```

Expected: PASS，且取消场景同时断言不存在成功终态和持久化变更。

- [ ] **Step 5: 提交跨层场景**

```bash
git add apps/cli/tests agent/composition/tests agent/features/config/src/core/connect/service_tests.rs
git commit -m "test(connect): 覆盖 Config Form 全链路场景"
```

---

### Task 10: 清理 Projection 命名并增加 Guard

**Files:**
- Modify: all Rust files reported by the naming scan, beginning with `apps/cli/src/command_contract_tests.rs`, `apps/cli/src/tui/**`, `agent/composition/tests/**`
- Create: `.agents/hooks/check-projection-naming.sh`
- Create: `.agents/hooks/tests/check-projection-naming.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 写 Guard 失败测试**

测试 fixture 必须包含：

```rust
struct InvalidProjectionName;
fn invalid_projection_name() {}
let invalid_projection_value = 1;
```

并验证注释、字符串和外部 JSON key 中出现 `projection` 不失败。

- [ ] **Step 2: 运行 Guard 失败测试**

```bash
bash .agents/hooks/tests/check-projection-naming.sh
```

Expected: 故意违规 fixture 被拒绝；无违规 fixture 通过。

- [ ] **Step 3: 重命名实际命中并实现扫描器**

按职责重命名：

- Connect `ConnectProjection` → `ConnectFormViewAssembler` 或删除；
- TUI `projection` 测试/变量 → `render_model`、`display_data`、`view_state` 或具体业务名称；
- Runtime/Composition 命中按真实来源和消费目标命名，不建立永久白名单。

扫描器只匹配 Rust 标识符 token，跳过注释、字符串和外部稳定数据；扫描生产、测试与 Guard fixture。

- [ ] **Step 4: 运行完整架构守卫**

```bash
bash .agents/hooks/check-architecture-guards.sh
```

Expected: 新 Guard 和既有 Guard 全部通过，且不存在旧 Runtime 专用重复扫描逻辑。

- [ ] **Step 5: 提交命名治理**

```bash
git add .agents/hooks docs/design/03-engineering/01-architecture-guards.md $(git diff --name-only -- '*.rs')
git commit -m "chore(guards): 禁止宽泛 Projection 命名"
```

---

### Task 11: 同步规范、设计文档和 Issue 门禁

**Files:**
- Modify: `specs/3.3-tui-cli.md`
- Modify: `specs/3.9-config-compat.md`
- Modify: `docs/superpowers/specs/2026-08-01-config-form-tui-connect-design.md`
- Modify: GitHub Issue 1457 body/checklist via `gh issue edit`

- [ ] **Step 1: 写文档差异清单**

逐项核对：Config 所有权、SDK Published Language、TUI 纯展示、Secret、workflow/session/revision、双宿主、Ratatui 场景、旧协议退役和 Projection Guard。

- [ ] **Step 2: 同步规范术语**

删除文档中把旧 `ConnectView` 当作最终 UI 协议的表述，改为通用 Config Form View；保留 Connect 领域状态机仅作为 Config 内部实现；补充实际文件路径和最终类型名。

- [ ] **Step 3: 更新 Issue 门禁**

使用 `gh issue edit 1457 --repo rushsinging/aemeath` 更新 checklist，仅将有验证证据的项标记完成；未实现的 L5 smoke 或后续钥匙链工作记录为明确延期，不伪造完成状态。

- [ ] **Step 4: 校验文档引用**

```bash
grep -R "ConnectProjection\|ConnectUiModel\|ConnectEffect" --line-number docs specs agent packages apps || true
git diff --check
```

Expected: 生产与规范不再引用退役 UI 类型；历史迁移说明若保留，必须明确为退役记录。

- [ ] **Step 5: 提交文档同步**

```bash
git add specs docs/superpowers/specs
git commit -m "docs(connect): 同步通用 Config Form 实现边界"
```

---

### Task 12: 最终验证、代码审查和 PR 前门禁

**Files:**
- Modify: PR Test plan / Issue checklist only when evidence is available

- [ ] **Step 1: 运行格式化与静态检查**

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: 全部通过；任何 warning 先修复，不用允许 warning 的方式绕过。

- [ ] **Step 2: 运行分层测试**

```bash
cargo test -p config
cargo test -p sdk
cargo test -p composition
cargo test -p cli
cargo test --workspace
```

Expected: 全部通过，并保留每层输出作为 PR Test plan 证据。

- [ ] **Step 3: 运行架构守卫和死代码检查**

```bash
bash .agents/hooks/check-architecture-guards.sh
git grep -nE 'ConnectProjection|ConnectUiModel|ConnectEffect|println!|print!' -- apps/cli/src/subcommand agent/features/config packages/sdk agent/composition || true
git status --short
```

Expected: Config Form 生产路径不存在手写 Connect frame；退役类型无生产引用；工作区只包含计划内变更。

- [ ] **Step 4: 运行真实 CLI smoke（具备交互终端时）**

```bash
cargo run -p cli -- connect
```

Expected: 页面在宽屏和窄屏保持对齐，resize 后完整重绘，取消恢复终端；记录 binary 路径、Git SHA、worktree、Session 和日志路径。

- [ ] **Step 5: 拉取最新主分支并执行 PR 前最终验证**

```bash
git pull origin main
cargo test --workspace
bash .agents/hooks/check-architecture-guards.sh
git diff origin/main...HEAD --stat
git status --short --branch
```

Expected: PR 分支不落后于 `origin/main`；所有 checklist 有证据或记录合理延期；不自动合并 PR。

- [ ] **Step 6: 提交最终验证记录**

```bash
git add -A
git commit -m "test(connect): 完成 Config Form 发布前验证"
```

---

## 计划自审

- Spec 覆盖：已覆盖 Config domain、Connect ACL、SDK PL、Composition、TUI reducer、Ratatui renderer、双宿主边界、Secret、首次启动、Provider Probe、命名 Guard、文档同步与 L1-L5 分层验证。
- 依赖顺序：Domain → ACL → SDK → Composition → TUI model → renderer → CLI 迁移 → 跨层场景 → Guard → 文档 → 最终门禁。
- TDD：每个核心行为均先写失败测试、运行确认失败，再写最小实现。
- 退役范围：旧 Connect UI DTO、文本 parser、`ConnectProjection`、raw-mode 手工 renderer 和旧 SDK UI 协议在同一迁移阶段删除，不保留双协议。
- 未扩张范围：系统钥匙链不实现；聊天内配置入口只保留可组合宿主接口，不注册新用户入口；不拆分 GitHub Issue。
