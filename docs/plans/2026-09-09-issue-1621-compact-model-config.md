# Issue 1621 实施计划：Compact 专用模型配置（动态解析 + 双窗口预算）

## 1. 目标与范围

支持为 Compact 指定专用模型，并修正"两个窗口被混为一个"的结构问题。

### 1.1 目标行为

1. 配置 `context.compact_model`（`<source>/<model>` selection，接受 `compactModel` 别名）。
2. **未配置**（缺省或空串）：compact 使用**当前会话模型**，`/model` 切换后下一次 compact 使用切换后的模型。
3. **已配置**：compact 使用该 selection 解析出的模型；每次 compact 从 committed config snapshot 解析，配置变更后下一次 compact 生效。
4. **双窗口预算**：Map/Reduce 分块目标按 compact 调用模型窗口；最终 summary 预算按主对话模型窗口。二者分离。
5. 解析失败（未知 source/model、缺 API key）返回明确错误，走既有 local fallback，**NEVER** 静默回退主模型。
6. 日志记录 compact 模型身份与预算来源（provider / model / 窗口 / 预算来源枚举），不记录 prompt 与响应正文。

### 1.2 范围外

- CLI 参数 `--compact-model`。
- Sub-agent 独立 compact 模型。
- 模型能力探测与自动降级探测。

## 2. 现状与根因

| 位置 | 现状 | 问题 |
|---|---|---|
| `agent/composition/src/runtime.rs` | 用 `initial_provider.binding()` 与 model 构造 `ProviderCompactGenerator` | 模型在启动期一次性绑定，无法配置、不随 `/model` 变化 |
| `agent/features/runtime/src/application/compact_generator.rs` | 固定 provider + model，`ReasoningLevel::Off`，输出上限 16384 | 同上 |
| `agent/features/context/src/adapters/compact_summary.rs` | `context_size` 同时驱动 `compact_chunk_target_tokens`（Map 分块）与 `summary_budget`（注入预算） | 两个语义不同的窗口被合并；换成不同模型后二者必须分离 |
| `agent/features/context/src/domain/token_budget.rs` | `summary_budget`、`compact_chunk_target_tokens` 均无独立来源 | 缺少显式预算来源类型 |
| `agent/features/runtime/src/application/client/accessors.rs` | `SessionModelState` 是会话模型唯一真相源，仅 Runtime 内部可见 | Composition 无法读取，generator 无法跟随 |

根因：Compact 的**模型选择**与**预算来源**都没有显式 owner，被隐式绑定到"启动时的主模型"。

## 3. 设计决策

1. **唯一 owner**：新增 Runtime 类型 `CompactModelResolver`，是"本次 compact 用哪个模型、窗口多大"的唯一解析入口。generator 与 Runtime 预算填充都必须经它，禁止旁路解析。
2. **唯一真相源**：会话当前模型仍由 `SessionModelState` 持有。Composition 通过 Runtime 公开的共享句柄（`SessionModelSlot`）读取，不复制状态。
3. **延迟绑定**：`SessionModelSlot` 由 Composition 创建（空），Runtime 在会话装配处绑定 `SessionModelState`；绑定后 resolver 与 Runtime 共享同一实例。
4. **双窗口显式化**：Context 侧引入显式预算来源，Map 分块按 compact 模型窗口，summary 预算按注入窗口。
5. **fail closed**：compact 模型窗口缺失时，chunk 预算取"与注入窗口一致"的下限，绝不放大小窗口模型的预算。
6. **配置解析失败不回退**：返回 typed 错误，compact 走 local fallback，日志 warn。

## 4. 分层任务

### 阶段 1 — 配置层（`agent/shared/src/config`）

- [ ] 1.1 写测试：`context.compact_model` 解析、`compactModel` 别名、空串/空白归一化为"未配置"、非法类型拒绝。
- [ ] 1.2 写测试：`ContextConfigPatch.compact_model` 合并保留未指定值，并可清除已有值。
- [ ] 1.3 实现 `ContextConfig.compact_model`、`ContextConfigPatch`、`ConfigSnapshot::context_compact_model()`。
- [ ] 1.4 更新 `specs/3.9-config-compat.md` 配置表（默认值、语义、覆盖优先级）。
- 验证：`cargo test -p share --lib`。

### 阶段 2 — Context 双窗口预算（`agent/features/context`）

- [ ] 2.1 写测试：注入窗口与 compact 窗口不同时，Map 分块目标按 compact 窗口计算。
- [ ] 2.2 写测试：summary 预算始终按注入窗口计算（与 compact 窗口无关）。
- [ ] 2.3 写测试：`CompactRequest`/`ManualCompactRequest` 未提供 compact 窗口时行为与现状一致（回归保护）。
- [ ] 2.4 实现显式预算来源类型与调用链签名调整。
- [ ] 2.5 文档：更新 `docs/design/02-modules/context-management/02-compact.md` 与 `03-token-budget.md`。
- 验证：`cargo test -p context`。

### 阶段 3 — Runtime 模型解析（`agent/features/runtime`）

- [ ] 3.1 写测试：resolver 在"已配置 / 未配置 / 解析失败 / 未绑定会话模型"下的目标模型、窗口与错误语义。
- [ ] 3.2 写测试：同 selection 复用缓存 binding，不重复构建（以 factory 调用计数断言）。
- [ ] 3.3 实现 `SessionModelSlot`（Composition 创建、Runtime 绑定）与 `CompactModelResolver`。
- [ ] 3.4 写测试：generator 按 resolver 结果选择 provider 与 model identity。
- [ ] 3.5 改造 `ProviderCompactGenerator` 使用 resolver；`Warn` 级记录解析结果与失败。
- [ ] 3.6 写测试：compact 请求携带 compact 模型窗口（自动路径与手动路径）。
- [ ] 3.7 在自动与手动 compact 请求构造处填充窗口（经 resolver，单一入口）。
- [ ] 3.8 日志：`aemeath:agent:runtime` target 记录模型身份、窗口与预算来源。
- 验证：`cargo test -p runtime`。

### 阶段 4 — Composition 装配（`agent/composition`）

- [ ] 4.1 写测试：装配后 generator 与 Run 预算使用同一 resolver。
- [ ] 4.2 装配 `SessionModelSlot` + `CompactModelResolver`，注入 Context generator 与 Runtime bootstrap 依赖。
- 验证：`cargo test -p composition --tests`。

### 阶段 5 — 场景验证与收尾

- [ ] 5.1 L4 场景测试：`/model` 切换后 compact 使用新模型。
- [ ] 5.2 L4 场景测试：配置 compact 模型后，Map 分块不超 compact 窗口且 summary 仍在注入预算内。
- [ ] 5.3 检查废弃路径：确认旧"固定 binding"构造方式无残留消费者。
- [ ] 5.4 文档对齐：`docs/design/02-modules/runtime/**`、`config/**` 与代码术语一致。
- [ ] 5.5 全量门禁：fmt、clippy、Context/Runtime/Composition 测试、架构守卫、workspace 门禁。

## 5. 测试策略（L0–L5）

- L1：配置解析/合并；预算分离纯函数。
- L2：resolver 三态与缓存；generator 模型选择。
- L3：`CompactGenerator` 契约（模型身份）、`CompactRequest` 预算字段语义。
- L4：模型切换与配置生效的端到端 compact 场景。
- L5：N/A（不涉及真实平台、浏览器、发布资产）。

## 6. 验证门禁

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p share --lib
cargo test -p context
cargo test -p runtime
cargo test -p composition --tests
cargo clippy -p share -p context -p runtime -p composition --all-targets -- -D warnings
cargo build -p context -p runtime -p composition
bash .agents/hooks/check-architecture-guards.sh --full
```

## 7. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| `ContextConfig` 当前 `derive(Copy)` | 加 `Option<String>` 后编译面变化 | 先移除 `Copy`，按编译器提示收敛受影响点 |
| `CompactRequest` 新增字段 | 大量测试构造点需要更新 | 字段提供默认语义（`None` = 与注入窗口一致），一次性更新构造点 |
| `RuntimeContext` 由专属 token 约束构造 | 新增依赖需走 `RuntimeContextFactory` | 依赖经 bootstrap deps 注入，禁止旁路构造 |
| 解析失败被误当成功 | 用户以为配置生效 | typed 错误 + warn 日志 + 失败走 local fallback |
| 双窗口计算错误导致请求超窗口 | compact 失败、退化为 local fallback | L2/L4 测试锁定 chunk 目标上界 |
| 新增配置项破坏旧配置解析 | 启动失败 | `#[serde(default)]` + 别名兼容 + 解析回归测试 |

## 8. 提交与交付

- 分支：`feat/1621-compact-model-config`（worktree 基于 `origin/main`）。
- 单一 PR，Squash merge。
- PR 使用 `Closes #1621`（仅在验收清单全部闭合时）。
