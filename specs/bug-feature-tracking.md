# Bug / Feature 追踪联动

**Scope**：无路径触发。任何 bug 修复或 feature 实现的流程约束；操作 GitHub Issues 时适用。
**主触发**：无（按场景）。
**次触发**：开始任何 bug 修复 / feature 实现；新增、更新或关闭 GitHub Issue；改 `docs/feature/specs/**` 设计稿。

## 仓库与工具

- **仓库**：`rushsinging/aemeath`。
- **CLI**：`gh`（`gh auth status` 通过）。所有 issue 操作（创建 / 查看 / 编辑 / 关闭 / 评论）**MUST** 用 `gh issue ... --repo rushsinging/aemeath`。
- **Label 体系**（创建 issue 时 **MUST** 应用）：
  - `kind:bug` —— bug 修复类。
  - `kind:feature` —— feature 实现类。
  - `priority:high` / `priority:medium` / `priority:low` —— 优先级（已知时加，未知可省）。
  - `migrated-from:docs` —— 仅历史迁移条目，**NEVER** 用于新建 issue。
- **标题**：直接写问题描述（一句中文），**NEVER** 加 `[Bug #N]` / `[Feature #N]` 前缀——`kind:*` label 已区分类型。

## 开工前

- **MUST** 先用 `gh issue list --repo rushsinging/aemeath --state open --label kind:bug,kind:feature --limit 50` 查看当前活跃条目，确认当前修改是否与已知条目相关或重复。
- 若发现重复 issue，**MUST** 评论到现有 issue 上而不是新建。

## 编号

- **NEVER** 手写 `Bug #N` / `Feature #N` 编号或前缀。GitHub 自动分配 issue 编号。issue body **NEVER** 含 `#<N>` 形式引用其他 issue（避免迁移历史中编号与 GitHub 编号混淆）；引用其他 issue 用 `gh issue view <N>` 在评论或 PR 描述中给链接。
- 历史迁移条目的原 docs 编号（`#1` ~ `#106` bug、`#1` ~ `#85` feature）作为标题前缀或正文子标题保留，便于阅读时识别对应原条目。body 顶部有 `<!-- Migrated from: <source> -->` 标记，可反查 `docs/bug/archived/<id>-<slug>.md` 或 `docs/active.md#<id>`。

## Issue Body 规范

- 标题：单句问题描述（80 字内）。
- Body 建议结构（**SHOULD** 完整覆盖；轻量 issue 可裁剪）：
  1. `## 现象` / `## 目标` —— bug 描述复现条件或 feature 要达成的效果。
  2. `## 根因` / `## 设计` —— bug 根因或 feature 关键设计决策。
  3. `## 修复 / 实现` —— 方案要点。
  4. `## 验证` —— 复现命令、测试方法、截图/录屏。
  5. `## 涉及路径` —— 文件 / 模块路径。
- 表格里摘要只回答"是什么"，**NEVER** 在 issue body 中复刻 80 字摘要+多行根因的 `active.md` 风格——GitHub Issue 鼓励长 body 完整描述。

## 工作流

- **Bug 修复 / feature 实现 MUST 使用 git worktree**（详见根 `AGENTS.md` 工作流约束）。
- **MUST** 修复 bug 时先添加重现该 bug 的测试用例，再提交修复代码（TDD）。
- **状态流转**（用 `gh issue edit` 更新 body 或 comment 表达）：
  - bug：`活动中` → `修复中` → `待确认` → 用户确认后关闭。
  - feature：`计划中` → `实现中` → `待 review` → 合并后关闭。
- **新建 issue 时 MUST**：
  1. 标题、body 完整。
  2. 至少打一个 `kind:*` label。
  3. 关联路径或 spec 在 body 末尾 `## 涉及路径` 段给出。
- **修改涉及已知 bug 时 MUST**：
  1. 在 PR 描述中引用 issue（如 `Closes #N`）。
  2. 合并后 `gh issue close N --repo rushsinging/aemeath --comment "已合并：<commit-sha>"`。
- **设计稿联动**：feature 类 issue **SHOULD** 配套 `docs/feature/specs/<file>.md` 设计稿。每份 spec 顶部已写 `> 对应 Issue: <url>`；修改 spec 时 **MUST** 同步更新该指针（issue 关 / 转 PR / 重开都要更新）。

## 关闭 / 重开

- 关闭前 **MUST** 确认：① 测试通过；② 已合并到 main；③ 用户确认。
- 关闭 comment **MUST** 含 commit SHA 或 PR 链接，便于审计。
- **NEVER** 用 `gh issue close --delete-branch`（仓库禁用 issue 关联删除分支）。

## 不属于本分片

- 改 `docs/feature/specs/**` 之外的 `docs/**`：按内容落到对应分片（`runtime.md` / `tools.md` 等）。
- 改 `specs/**` 自身：按改动内容分片（与本分片无关）。
