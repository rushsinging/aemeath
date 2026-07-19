# 工作流约束

> 路径触发：无（不在代码路径下）。
> 场景触发：任何 bug 修复 / feature 实现 / PR 创建 / 发版 / Hook 阻断处理。

## Bug / Feature 执行流程

bug / feature 追踪改在 GitHub Issues（仓库 `rushsinging/aemeath`），按以下步骤执行，**NEVER** 跳过：

新建 issue 时 **SHOULD** 使用 `.github/ISSUE_TEMPLATE/bug.yml` 或 `feature.yml`，以保证 `kind:*`、`area:*`、`priority:*` 标签由 `.github/workflows/auto-labeler.yml` 一致地应用。创建 PR 时 **SHOULD** 使用 `.github/pull_request_template.md` 填写 Summary、Refs、Breaking change、Test plan。

1. **阅读 Issue 并建立门禁清单**：用 `gh issue view <编号> --repo rushsinging/aemeath` 拉取 issue 标题、labels、完整 body。
   - **MUST** 检查 issue 是否关联 milestone。未关联 milestone 的 issue **MUST** 先提醒用户关联，**NEVER** 在无 milestone 的情况下直接开 worktree 修改。
   - **MUST** 识别 issue body 中全部 checklist / check 项，并将其作为开发、验证和 PR 前的门禁清单；**NEVER** 只读取标题或验收摘要后跳过清单。
2. **定位问题并给出方案**：阅读相关源码，定位根因，**MUST** 向用户输出可执行的修复/实现方案（含改动范围、根因分析、验证计划）。方案 **MUST** 包含测试策略——按 `docs/design/03-engineering/04-testing-and-coverage.md` 的 L0-L5 六层模型选择覆盖证据；bug 修复 MUST 先写复现测试，feature 实现 MUST 先写 TDD 测试。复杂改动 **MUST** 调用 `superpowers:writing-plans` 制定详细计划。
3. **等待用户明确同意**：在获得用户的明确书面同意（如"同意"、"开始改"）前，**NEVER** 调用 Edit/Write/Bash 等会修改文件或系统状态的工具。
4. **执行与验证**：在 worktree 中实施，worktree **MUST** 基于 `origin/main` 最新 commit 创建。实施过程中 **MUST** 逐项执行并持续核验门禁清单；通过编译、测试、clippy 验证后 PR 合入 `main`。
   - 完成开发及创建 PR 前，**MUST** 确认所有 check 项均已完成；无法完成或不适用的项目 **MUST** 在 issue 或 PR 中记录可验证的合理理由、影响和后续处理。
   - 存在未完成且无合理理由的 check 项时，**NEVER** 宣称完成或创建 PR。
5. **用户确认后关闭 Issue**：agent **NEVER** 自行关闭 issue。

修复 bug 或实现 feature 时，**MUST** 做根因层面的修正（fact-check），而不是只做最小化补丁绕过症状。

标签约定：
- `kind:*`：`kind:bug`（缺陷）、`kind:feature`（功能）、`kind:rfc`（重大设计问题）。
- `area:*`：根据改动路径自动标注（映射见 `.github/area-map.json`）。
- `priority:*`：`priority:high`、`priority:medium`、`priority:low`。

## Milestone / Release Gate 管理

Milestone 跟 release 版本走，用于表达某个版本要交付的**可验收能力包**，**NEVER** 作为 issue 分类桶。

1. **命名规则**：milestone 标题 **MUST** 使用 `vX.Y.Z — 能力目标`。
2. **范围规则**：每个 issue **SHOULD** 只归属一个 milestone；跨版本或长期方向的 RFC / backlog **SHOULD NOT** 进入 milestone。
3. **Release Gate issue**：每个 milestone **MUST** 有且只有一个验收 issue，标题格式为 `[Release Gate] vX.Y.Z — 能力目标`。
4. **必有 issue 类型**：除功能 / bug 执行 issue 外，每个 milestone **MUST** 包含 Release Gate issue、收尾退役 issue、大文件拆分 issue。
5. **关联规则**：纳入版本范围的执行 issue **MUST** 设置同一个 milestone，并在 Release Gate issue 的关联清单中出现。
6. **进度维护**：执行 issue 合入、移出或发现阻断时，**MUST** 更新 Release Gate issue 的 checklist / 阻断项 / out-of-scope。

Release Gate issue 模板见仓库 `.github/ISSUE_TEMPLATE/`。

## 大型工作的拆分与跟踪（GitHub Sub-issues）

跨多个子系统、需多个 PR 才能完成的大型工作，**MUST** 使用 GitHub 原生 parent / sub-issue 层级组织，**NEVER** 塞进单一 issue 或 PR：

1. **父 Issue 承载大纲**：整体目标、范围边界、依赖图、阶段状态和完成定义。父 Issue **NEVER** 直接承载代码 PR。
2. **使用原生 Sub-issues**：每个可独立验证、独立 PR 的交付单元 **MUST** 创建为 GitHub 原生 sub-issue。
3. **标注依赖与并行性**：依赖关系 **MUST** 使用 GitHub 原生 blocked-by / blocking 关系。
4. **拆分规模**：每层直接 sub-issues **SHOULD** 不超过 10 个。超过时 **MUST** 按稳定模块或能力边界增加中间父 Issue。
5. **必有收尾能力**：大型工作 **MUST** 覆盖 Guard + Verify、收尾退役、大文件拆分三类交付。
6. **依赖顺序**：sub-issues **MUST** 按领域模型 → Port / PL → Adapter → 消费方 → Guard / 退役的方向拆分。依赖方向严格从内到外。

## 开发环境（Cargo build-dir / Git hooks / sccache）

- 开发环境 **MUST** 使用 Cargo 1.91+，并通过 `scripts/setup-dev-env.sh --check` 验证。
- `core.hooksPath` **MUST** 配置为主 checkout `.cargo/hooks` 的绝对路径。
- `post-checkout` **MUST** 生成 worktree-local `.cargo/config.toml`，将 `target-dir` 和 `build-dir` 设为 `~/.cache/aemeath-target/<分支标签>-<worktree 路径哈希>`。
- 开发环境 **SHOULD** 安装 sccache，并在 `~/.cargo/config.toml` 配置 `rustc-wrapper = "sccache"`。
- worktree 构建缓存 **MUST** 使用 `scripts/clean-worktree-targets.sh [--dry-run] [--keep-current] [--yes] [--max-size-gb N]` 清理。
- 清理脚本 **NEVER** 自动删除活跃 worktree 的构建缓存。

## Git 工作流

日常开发在 `main` 上进行；`release/vX.Y.Z` 分支仅在发版时从 `origin/main` 切出，用于版本发布与发版后的维护（hotfix）。

- **MUST** 所有 feature / bugfix 在独立 worktree 中开发，worktree 基于 `origin/main` 最新 commit 创建；**NEVER** 直接 push 到 `main` 或 `release/*`。
- **MUST** feature / bugfix 分支完成后通过 **Pull Request** 提交到 `main`。
- **MUST** 合并 PR 的策略按 PR 方向区分：
  - **feature/bugfix → main**：使用 **Squash merge**。
  - **main → release**（发版）：使用 **Merge commit**（`--no-ff`，非 squash、非 rebase）。
  - **NEVER** 使用 rebase merge。
- **MUST** agent 默认不得合并 PR；PR 创建后应等待用户 review。只有同时满足以下条件时，agent **MAY** 代用户执行合并：
  1. 用户在**当前会话**中明确授权合并**具体 PR 编号或 URL**；“继续”“处理一下”等泛化指令不构成授权。
  2. 授权只对该 PR 的当前 head commit 有效，**NEVER** 跨 PR、跨会话或在授权后新增提交时沿用；head 变化后 **MUST** 重新取得授权。
  3. 合并前 **MUST** 用 GitHub 查询确认 base/head、非 Draft、mergeable、merge state 与 required checks；存在失败、pending、冲突或 branch protection 阻断时 **NEVER** 绕过。
  4. **MUST** 严格使用本节规定的方向策略：feature/bugfix → main 仅 Squash merge，main → release 仅 Merge commit；**NEVER** 使用 admin bypass、rebase merge 或修改保护规则。
  5. 合并后 **MUST** 查询 PR 状态与目标分支结果，确认实际合入；失败时报告真实状态，**NEVER** 重复或改用其他策略静默重试。
- **MUST** 创建 PR 前，在 worktree 分支上执行 `git pull origin main` 拉取最新主分支。
- **MUST** 发版时从 `origin/main` 最新 commit 切出 `release/vX.Y.Z` 分支，使用 **Merge commit**（`--no-ff`）将 `main` 合入 `release/vX.Y.Z` 后，在 release 分支 HEAD 打 `vX.Y.Z` tag 触发发版 workflow。

### 分支保护规则（Rulesets）

`main` 分支的保护规则通过 **GitHub Repository Rulesets** 强制，配置固化为 `.github/rulesets/*.json`（唯一真相源），由 `.github/rulesets/setup.sh` 同步到 GitHub 服务端。规则包含：

- **pull_request**：要求 PR 合并，dismiss stale reviews
- **required_status_checks**：要求 `Coverage` check 通过
- **deletion**：禁止删除 `main`
- **non_fast_forward**：禁止 force push

新仓库初始化或规则变更后，**MUST** 执行 `bash .github/rulesets/setup.sh` 同步。Rulesets 优先级高于旧 branch protection；迁移时先确认无冲突。

## 代码修改后检查

每次完成代码修改后（含 bug 修复、feature 实现、重构），**SHOULD** 检查是否产生了应当被移除的旧代码、废弃路径、过期兼容层、仅被测试引用的死代码。发现时 **MUST** 向用户报告并建议清理方案，**NEVER** 在知情的情况下让待退役代码静默遗留。

## 发版

发版通过 push `v*` tag 触发 `.github/workflows/release.yml` 自动完成。**MUST** 遵守：

- **MUST** 由用户明确指定版本号，agent **NEVER** 自行决定发版或推演版本号。
- **MUST** tag 打在 `release/vX.Y.Z` 分支上（`main` 合入 release 后的 HEAD）。
- **MUST** 使用轻量 tag，格式 `vX.Y.Z`。**NEVER** 改 `Cargo.toml` 的 `workspace.version`（占位符 `0.0.0`）；实际版本由 `release.yml` 的 `build` job 显式注入。
- **MUST** push tag 前先向用户输出方案并等待确认；**NEVER** 未经确认直接 push tag。
- **MUST** push 后用 `gh run list --workflow=release.yml` 监控全部通过；任一失败 **MUST** 排查并报告。
- release notes 由 `generate_release_notes: true` 自动从 PR 生成，agent **NEVER** 手写发版说明。
- **MUST** 用 `gh release view vX.Y.Z` 确认 Release 已发布且包含 aarch64 / x86_64 tar.gz + checksums.txt 三个 asset。

## Hook 阻断处理

工作中若遇到 hook 阻断（例如 PreToolUse 阻止 Edit/Write）：

1. **MUST** 先止血：立即切换到正确的工作上下文（如进入 git worktree），让用户请求的原始操作能够继续执行。
2. **MUST** 向用户报告：发生了什么阻断、阻断原因、以及采取了什么措施来处理。
