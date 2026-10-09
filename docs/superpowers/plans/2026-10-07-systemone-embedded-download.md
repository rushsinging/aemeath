# System One 批次 2：embedded llama.cpp 与模型下载 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 System One 生产评分切换为 macOS arm64 上的 embedded llama.cpp + Q8_0 GGUF，并新增显式的 `aemeath systemone download` 模型安装命令；启动不下载，HTTP 仅保留测试/eval用途。

**Architecture:** `systemone` BC 提供模型 manifest、下载用例、PointerHead 数学和 embedded `ScoringPort`。llama.cpp model/context/batch 全部驻留专用 worker thread，异步 facade 只传递 owned token ids 与 owned probabilities。Composition 负责装配与启动通知，CLI 只解析命令并转发，不直接读取文件系统或环境变量。

**Tech Stack:** Rust 2021、Tokio、`llama-cpp-2` 0.1.158、reqwest（仅下载与测试 HTTP adapter）、serde/sha2/tempfile、clap、现有 `ScoringPort`/Composition/SDK/CLI 架构。

---

## 文件地图

- **Modify:** `agent/shared/src/config/adapters/constants.rs`、`agent/shared/src/config/adapters/paths.rs`、对应 paths tests；增加 `~/.agents/models/systemone` 单一路径来源。
- **Modify:** `packages/global/utils/src/lib.rs` 与 tests；提升公共 SHA-256 helper；`agent/features/update/src/service/checksum.rs` 改为委托公共 helper。
- **Create:** `agent/features/systemone/src/domain/pointer_head.rs`、`pointer_head_tests.rs`、`model_manifest.rs`、`model_manifest_tests.rs`。
- **Create:** `agent/features/systemone/src/application.rs`、`src/application/download_model.rs`、对应 tests。
- **Create:** `agent/features/systemone/src/adapters/model_assets.rs`、`fetch_http.rs`、`embedded.rs`、`llama_worker.rs` 与对应 tests；修改 `Cargo.toml`、`adapters.rs`、`domain.rs`、`ports.rs`、`lib.rs`。
- **Modify:** `agent/shared/src/config/domain/scoring.rs`、`merge.rs`、`agent/features/config/src/adapters.rs` 及测试；移除生产 HTTP URL/model/timeout 配置。
- **Modify:** `agent/composition/src/runtime.rs`、`app.rs`、`lib.rs`、registry 数据；新增 `agent/composition/src/systemone.rs` 与 wiring tests。
- **Modify:** `apps/cli/src/args.rs`、`main.rs`、`subcommand.rs`；新增 `apps/cli/src/subcommand/systemone_command.rs` 与 CLI/启动通知测试。
- **Create:** `eval/system-one/harness/export_parity_fixture.py`、`agent/features/systemone/tests/fixture_parity.rs`、`embedded_parity.rs`；生成 Q8_0 fixture 和结果报告。
- **Modify:** `docs/design/02-modules/systemone/01-systemone-scoring.md`（已落盘的设计为实现依据；实施中只补实际偏差）。

---

### Task 1: 添加 System One 模型路径单一真相源

**Files:**
- Modify: `agent/shared/src/config/adapters/constants.rs`
- Modify: `agent/shared/src/config/adapters/paths.rs`
- Test: `agent/shared/src/config/adapters/paths_tests.rs`

- [ ] **Step 1: 写失败测试**

在现有 `TestEnvGuard` 测试模块中添加一个临时 agents 根目录，并断言 `systemone_models_dir()` 等于 `<agents>/models/systemone`；再将 `AEMEATH_AGENTS_DIR` 设为空白，断言回退到 home agents 根下的同一相对布局。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p share config::adapters::paths -- --nocapture`
Expected: 编译失败，找不到 `systemone_models_dir`。

- [ ] **Step 3: 实现路径函数**

在 constants 中增加 `MODELS_DIR_NAME` 与 `SYSTEMONE_DIR_NAME`；在 paths 中新增 `global_models_dir()` 和 `systemone_models_dir()`，都只调用现有 `global_agents_dir()`，不得重复读取环境变量。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p share config::adapters::paths -- --nocapture`
Expected: PASS，环境变量覆盖、缺失与空白三种情况均通过。

- [ ] **Step 5: 提交**

```bash
git add agent/shared/src/config/adapters/constants.rs agent/shared/src/config/adapters/paths.rs agent/shared/src/config/adapters/paths_tests.rs
git commit -m "feat(config): add System One model cache path"
```

---

### Task 2: 提升公共 SHA-256 helper 并复用到 update

**Files:**
- Modify: `packages/global/utils/src/lib.rs` 与测试文件
- Modify: `agent/features/update/src/service/checksum.rs` 与测试
- Modify: `.agents/architecture-guard-registry.json`

- [ ] **Step 1: 写失败测试**

为 `utils::sha256_hex` 添加 `hello` 与空字节切片的已知 SHA-256 断言；保留 update 现有 checksum 测试并先将实现替换为调用不存在的公共函数。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p utils -p update checksum`
Expected: 失败，公共 `sha256_hex` 不存在。

- [ ] **Step 3: 实现并复用**

在 utils 暴露 `pub fn sha256_hex(data: &[u8]) -> String`；update 的 checksum 模块删除重复哈希实现，委托 utils；同步 workspace dependency matrix 使 update → utils 合法。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p utils -p update`
Expected: PASS，update 原有 checksum 解析与哈希断言保持通过。

- [ ] **Step 5: 提交**

```bash
git add packages/global/utils agent/features/update .agents/architecture-guard-registry.json
git commit -m "refactor(utils): share sha256 helper with update"
```

---

### Task 3: 实现 PointerHead 领域数学

**Files:**
- Create: `agent/features/systemone/src/domain/pointer_head.rs`
- Create: `agent/features/systemone/src/domain/pointer_head_tests.rs`
- Modify: `agent/features/systemone/src/domain.rs`

- [ ] **Step 1: 写失败测试**

覆盖以下固定行为：`q = Wq*h_decide+bq`、`k = Wk*h_option+bk`、`dot(k,q)/sqrt(256)/temperature`；softmax 使用减最大值避免溢出；权重维度不是 `[256,1024]` 时拒绝；非有限 hidden/logit 返回错误；argmax 与 Python 参考固定向量一致。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone pointer_head`
Expected: 编译失败，模块与类型不存在。

- [ ] **Step 3: 实现纯数学类型**

实现无 llama.cpp 依赖的 `PointerHeadWeights`、`PointerHead` 与 `score_options`，只接收 owned/borrowed `f32` slices，输出 `Vec<f32>` 概率；加载层不放在 domain。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p systemone pointer_head`
Expected: PASS，维度、有限性、softmax、温度与 argmax 测试通过。

- [ ] **Step 5: 提交**

```bash
git add agent/features/systemone/src/domain.rs agent/features/systemone/src/domain/pointer_head.rs agent/features/systemone/src/domain/pointer_head_tests.rs
git commit -m "feat(systemone): add PointerHead scoring math"
```

---

### Task 4: 实现模型 manifest 领域校验

**Files:**
- Create: `agent/features/systemone/src/domain/model_manifest.rs`
- Create: `agent/features/systemone/src/domain/model_manifest_tests.rs`
- Modify: `agent/features/systemone/src/domain.rs`、`ports.rs`

- [ ] **Step 1: 写失败测试**

覆盖 manifest 缺字段、SHA-256 格式错误、文件大小不符、hidden size 非 1024、pointer dimension 非 256、temperature 非有限/非正、当前平台不在支持列表等 fail-closed 行为。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone model_manifest`
Expected: 编译失败或测试失败，因为 manifest 类型和校验函数不存在。

- [ ] **Step 3: 实现校验类型**

定义 `ModelManifest`、资产条目、平台标识与校验错误；为 `ModelAssetPort` 约定 `InstalledAssets`、`Missing`、`Invalid` 三态，所有结构与约束从 manifest 单一来源读取。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p systemone model_manifest`
Expected: PASS，所有非法 manifest 被拒绝。

- [ ] **Step 5: 提交**

```bash
git add agent/features/systemone/src/domain/model_manifest.rs agent/features/systemone/src/domain/model_manifest_tests.rs agent/features/systemone/src/domain.rs agent/features/systemone/src/ports.rs
git commit -m "feat(systemone): validate model manifests"
```

---

### Task 5: 实现模型资产本地解析、哈希校验和原子安装

**Files:**
- Create: `agent/features/systemone/src/adapters/model_assets.rs`
- Create: `agent/features/systemone/src/adapters/model_assets_tests.rs`
- Modify: `agent/features/systemone/src/adapters.rs`

- [ ] **Step 1: 写失败测试**

使用 tempfile 验证：缓存缺失返回 `Missing`；有效目录返回 `InstalledAssets`；SHA 错误、manifest 不匹配和半成品目录返回 `Invalid`；临时目录安装成功后才出现最终 revision 目录；失败不覆盖旧有效版本。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone model_assets`
Expected: 编译失败或断言失败，因为本地资产 adapter 不存在。

- [ ] **Step 3: 实现本地资产 adapter**

实现 revision 目录解析、文件长度/SHA-256/manifest 结构校验和临时目录命名；写入完成后对目录执行原子 rename；禁止将不完整目录识别为有效安装。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p systemone model_assets`
Expected: PASS，缺失、有效、损坏、半成品、不覆盖五类测试通过。

- [ ] **Step 5: 提交**

```bash
git add agent/features/systemone/src/adapters.rs agent/features/systemone/src/adapters/model_assets.rs agent/features/systemone/src/adapters/model_assets_tests.rs
git commit -m "feat(systemone): add verified model asset storage"
```

---

### Task 6: 实现手动下载 application service

**Files:**
- Create: `agent/features/systemone/src/application.rs`
- Create: `agent/features/systemone/src/application/download_model.rs`
- Create: `agent/features/systemone/src/application/download_model_tests.rs`
- Create: `agent/features/systemone/src/adapters/fetch_http.rs`
- Modify: `agent/features/systemone/src/ports.rs`、`Cargo.toml`

- [ ] **Step 1: 写失败测试**

通过 fake `ArtifactFetcherPort` 验证：有效缓存不发网络请求；下载三项资产并完成校验后安装；网络失败、长度错误、SHA 错误、结构错误均返回失败且无可用半成品；已有有效 revision 不被覆盖；失败命令结果可映射为非零退出。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone download_model`
Expected: 编译失败或测试失败，因为下载 application 不存在。

- [ ] **Step 3: 实现 download 用例**

定义 `ModelDownloadService` 与 `DownloadOutcome`；application 只编排 manifest/fetcher/local installer，不读取 CLI 参数之外的全局状态。HTTP fetcher 使用 reqwest 流式写入临时文件或 owned bytes；最终安装只能经 model assets adapter。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p systemone download_model`
Expected: PASS，fake fetcher 覆盖幂等、fail-closed 和原子安装行为。

- [ ] **Step 5: 提交**

```bash
git add agent/features/systemone/src/application.rs agent/features/systemone/src/application agent/features/systemone/src/adapters/fetch_http.rs agent/features/systemone/src/ports.rs agent/features/systemone/Cargo.toml
git commit -m "feat(systemone): add explicit model download service"
```

---

### Task 7: 配置 HTTP 字段退役并门控测试 adapter

**Files:**
- Modify: `agent/features/systemone/Cargo.toml`、`adapters.rs`、`lib.rs`
- Modify: `agent/shared/src/config/domain/scoring.rs`、`merge.rs`
- Modify: `agent/features/config/src/adapters.rs` 与测试
- Modify: `agent/features/systemone/tests/kev_baseline.rs`

- [ ] **Step 1: 写失败测试**

先更新配置测试，断言保留四个场景开关、移除 URL/model/timeout patch；新增合同测试断言默认 systemone 构建不注册 Jev HTTP adapter，而显式 HTTP test feature 仍能运行既有基线。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p share -p config -p systemone`
Expected: 失败，旧字段和无 feature 的 HTTP 测试仍被引用。

- [ ] **Step 3: 实现配置与 feature 门控**

从生产 `ScoringConfig`、patch merge 和 env adapter 移除 HTTP 连接配置；在 systemone Cargo features 中将 Jev HTTP adapter 设为测试/eval feature；`kev_baseline` 声明 required feature；保持下载 fetcher 所需 HTTP 依赖与评分 HTTP adapter 语义分离。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p share -p config -p systemone` 与 `cargo test -p systemone --features http-adapter --test kev_baseline -- --ignored`
Expected: 默认构建不含生产 HTTP factory；显式 HTTP 基线可运行或在无服务时明确被 ignore/环境门禁跳过。

- [ ] **Step 5: 提交**

```bash
git add agent/features/systemone agent/shared/src/config/domain/scoring.rs agent/shared/src/config/domain/merge.rs agent/features/config/src/adapters.rs
git commit -m "refactor(systemone): retire HTTP scoring configuration"
```

---

### Task 8: 实现 llama.cpp worker 与 embedded ScoringPort

**Files:**
- Create: `agent/features/systemone/src/adapters/llama_worker.rs`
- Create: `agent/features/systemone/src/adapters/llama_worker_tests.rs`
- Create: `agent/features/systemone/src/adapters/embedded.rs`
- Create: `agent/features/systemone/src/adapters/embedded_tests.rs`
- Modify: `agent/features/systemone/Cargo.toml`、`adapters.rs`、`ports.rs`

- [ ] **Step 1: 写失败测试**

先在 macOS arm64 embedded feature 下写 worker contract tests：Q8_0 GGUF 加载成功；context 使用 embeddings=true/pooling none；选定 token 的 `embeddings_ith` 长度为 1024 且 finite；同一 row 清 KV 后重复评分结果稳定；模型缺失/维度不符/初始化失败映射为启动禁用状态；运行期失败映射 `ScoringUnavailable`。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone --features embedded llama_worker -- --nocapture`
Expected: 失败，因为 worker 与 embedded adapter 尚未实现。

- [ ] **Step 3: 实现 worker**

在 worker 专用线程内依次初始化 `LlamaBackend`、`LlamaModel::load_from_file`、`LlamaContextParams::with_embeddings(true).with_pooling_type(LlamaPoolingType::None)`、`LlamaBatch`；每个需要读取的 token 以 `logits=true` 加入 batch，decode 后立即复制 `embeddings_ith` 为 `Vec<f32>`，再 clear KV/batch。不得把 `LlamaContext` 或 `LlamaBatch` 发送到 Tokio task。

- [ ] **Step 4: 实现 embedded facade**

实现 `ScoringPort`：将 published language 映射为 kev causal row，向 bounded channel 发送 owned request，收回 owned hidden vectors，调用 domain PointerHead 生成对应 `ScoringAnswer`；初始化失败不构造 port，单次 worker 错误返回 `ScoringUnavailable`。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p systemone --features embedded llama_worker embedded`
Expected: macOS arm64 本地模型存在时 PASS；无模型时测试明确 skip/提示 `aemeath systemone download`，不触发下载。

- [ ] **Step 6: 提交**

```bash
git add agent/features/systemone
 git commit -m "feat(systemone): add embedded llama.cpp scoring worker"
```

---

### Task 9: 收窄 systemone factory 并接入 composition

**Files:**
- Modify: `agent/features/systemone/src/lib.rs`
- Modify: `agent/composition/src/runtime.rs`、`app.rs`、`lib.rs`
- Create: `agent/composition/src/systemone.rs` 与 tests
- Modify: `.agents/architecture-guard-registry.json`

- [ ] **Step 1: 写失败测试**

添加 composition 装配矩阵测试：场景全关不读模型且无 notice；场景开启+有效模型构造 embedded port；模型缺失返回 notice 且无 port；HTTP adapter 不出现在 production composition。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p composition systemone`
Expected: 编译失败，因为 factory 仍接收 HTTP 参数且 composition 没有 embedded wiring/notice。

- [ ] **Step 3: 收窄 factory**

将 `wire_scoring_port(base_url, model, timeout, scoring_dir)` 改为不暴露 HTTP 的 `ScoringWiring` 输入和 `WireScoringOutcome` 输出；保留 `Embedded → Calibrated → Audited` 装配，审计 revision 来自 manifest。

- [ ] **Step 4: 接入 composition**

在 runtime scoring 开关门内调用新 factory；扩展 bootstrap 的 typed `startup_notices`；新增 composition systemone download wiring，确保 CLI 不直接访问 fs/env；同步 construction/layout/dependency registry。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p composition systemone`、`cargo run -p xtask -- guard-registry check`
Expected: 装配矩阵与 registry 检查通过。

- [ ] **Step 6: 提交**

```bash
git add agent/features/systemone agent/composition .agents/architecture-guard-registry.json
git commit -m "feat(composition): wire embedded System One scoring"
```

---

### Task 10: 添加启动模型缺失 typed notice

**Files:**
- Modify: `agent/composition/src/app.rs`、`runtime.rs`
- Modify: `apps/cli/src/chat.rs`、`chat/no_tui.rs`
- Test: composition bootstrap tests、CLI no-TUI/TUI scenario tests

- [ ] **Step 1: 写失败测试**

断言：模型缺失时 bootstrap 携带一条包含 `aemeath systemone download` 的 notice；no-TUI 启动只打印一次；TUI system notice 只渲染一次；场景全关时 notice 为空。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p composition -p cli systemone_notice`
Expected: 失败，因为 bootstrap 没有 typed startup notice 消费链路。

- [ ] **Step 3: 实现 bootstrap 透传**

定义稳定的 `StartupNotice` published value，由 composition 生成并通过 bootstrap 透传；CLI/TUI 只渲染消息，不读取 ConfigReader、环境变量或模型目录。fatal error 不参与此路径。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p composition -p cli systemone_notice`
Expected: PASS，TUI/no-TUI 均显示一次，主聊天仍可启动。

- [ ] **Step 5: 提交**

```bash
git add agent/composition apps/cli
 git commit -m "feat(cli): report missing System One model without blocking chat"
```

---

### Task 11: 添加 CLI `aemeath systemone download`

**Files:**
- Modify: `apps/cli/src/args.rs`、`main.rs`、`subcommand.rs`
- Create: `apps/cli/src/subcommand/systemone_command.rs`
- Test: `apps/cli/src/args_tests.rs`、`command_contract_tests.rs`、`agent/composition/tests/systemone_download_wiring.rs`

- [ ] **Step 1: 写失败测试**

添加 clap 解析测试：`aemeath systemone download` 成功；`aemeath systemone` 缺少子命令失败；command contract 断言 CLI command handler 只委托 composition，不调用 fs/env/reqwest。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p cli systemone_download`
Expected: 编译失败或解析失败，因为 nested command 尚未注册。

- [ ] **Step 3: 实现 CLI 转发**

在 `Commands` 中新增 `Systemone { command: SystemoneCommands }` 和 `SystemoneCommands::Download`；main 只将参数转发到 `composition::systemone::run_systemone_download`；subcommand 文件不包含路径解析、HTTP 或资产校验逻辑。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p cli systemone_download`、`cargo run -p cli -- systemone download --help`
Expected: 解析测试通过，帮助文本展示 nested command。

- [ ] **Step 5: 提交**

```bash
git add apps/cli agent/composition/tests/systemone_download_wiring.rs
git commit -m "feat(cli): add systemone model download command"
```

---

### Task 12: 导出 79-case fixture 并添加 Rust 回归门禁

**Files:**
- Create: `eval/system-one/harness/export_parity_fixture.py`
- Create: `eval/system-one/fixtures/parity_q8/79_cases.jsonl`、`manifest.json`
- Create: `agent/features/systemone/tests/fixture_parity.rs`、`embedded_parity.rs`
- Modify: `agent/features/systemone/Cargo.toml`

- [ ] **Step 1: 写失败测试**

先添加 fixture reader 与 PointerHead/argmax 恒等测试，断言 fixture 行数 79；embedded integration test 在缺少本地模型时只报告 skip，在 macOS arm64 + embedded feature + 显式运行时执行 79-case argmax 门禁。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p systemone --test fixture_parity`
Expected: 失败，fixture 尚未生成。

- [ ] **Step 3: 生成可再生 fixture**

实现导出器，复用现有 `parity_gguf.py` payload/encode 逻辑，写入 row token ids、golden probabilities、golden argmax、选项数量与 manifest 来源；固定生成 revision 与 case count，避免将临时服务地址写入生产代码。

- [ ] **Step 4: 运行 fixture 测试**

Run: `cargo test -p systemone --test fixture_parity`
Expected: PASS，79 行完整读取，纯数学/argmax 对拍通过。

- [ ] **Step 5: 运行 embedded 真实门禁**

Run: `cargo test -p systemone --features embedded --test embedded_parity -- --ignored --nocapture`
Expected: macOS arm64 且执行 `aemeath systemone download` 后 PASS，argmax ≥99%；模型缺失时明确提示并退出为环境 skip，不下载。

- [ ] **Step 6: 提交**

```bash
git add eval/system-one/harness/export_parity_fixture.py eval/system-one/fixtures agent/features/systemone/Cargo.toml agent/features/systemone/tests
git commit -m "test(systemone): add Rust embedded parity fixtures"
```

---

### Task 13: 更新架构 registry 并运行完整验证

**Files:**
- Modify: `.agents/architecture-guard-registry.json`
- Modify: `docs/design/02-modules/systemone/01-systemone-scoring.md`（仅记录实际偏差）

- [ ] **Step 1: 检查 registry 差异**

确认 construction `wire_scoring_port`、composition top-level `systemone.rs`、workspace matrix `systemone`/`update→utils` 和 feature 测试入口均有对应数据规则，不添加脚本豁免。

- [ ] **Step 2: 运行 guard registry**

Run: `cargo run -p xtask -- guard-registry check`
Expected: PASS。

- [ ] **Step 3: 运行 Rust 格式与测试**

Run: `cargo fmt --all -- --check`
Expected: PASS。

Run: `cargo test -p systemone -p composition -p config -p share -p update -p cli`
Expected: PASS；HTTP adapter 只有显式 test feature 时参与测试。

- [ ] **Step 4: 运行 clippy**

Run: `cargo clippy -p systemone -p composition -p config -p share -p update -p cli --all-targets --all-features -- -D warnings`
Expected: PASS。

- [ ] **Step 5: 运行架构守卫**

Run: `cargo run -p xtask -- guard --fast`
Expected: PASS。

- [ ] **Step 6: 运行 workspace 测试**

Run: `cargo test --workspace --all-targets --locked`
Expected: PASS；若环境缺少 macOS embedded 模型，只允许 embedded ignored 门禁跳过，不能吞掉普通测试失败。

- [ ] **Step 7: 提交验证结果**

```bash
git add .agents/architecture-guard-registry.json docs/design/02-modules/systemone/01-systemone-scoring.md
git commit -m "chore(systemone): finalize embedded model installation gates"
```

---

## 验收矩阵

| 验收项 | 证据 |
|---|---|
| 启动不下载 | composition/CLI 断言 + 下载器 mock 计数为 0 |
| 手动下载命令 | clap 解析、composition wiring、fake fetcher、真实 CLI help |
| 幂等与 fail-closed | model asset/application tests |
| 缺失模型不阻断主循环 | bootstrap startup notice + no-TUI/TUI 场景测试 |
| 生产不构造 HTTP adapter | composition contract + feature gate |
| HTTP 仅测试/eval | `http-adapter` required feature 的基线测试 |
| PointerHead 数值 | domain 固定向量与 79-case fixture |
| Q8_0 对分 | embedded parity ≥99%；历史 f16/Q8_0 结果已为 100% |
| macOS arm64 内存 | embedded 真实门禁记录 ≤1.5GB |
| 其他平台 | 默认 workspace compile/test 不启用 embedded 运行路径 |
| 架构合规 | guard-registry、guard fast、workspace test 全绿 |
