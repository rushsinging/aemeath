# Bug / Feature 追踪联动

**Scope**：无路径触发。任何 bug 修复或 feature 实现的流程约束；操作 GitHub Issues 时适用。
**主触发**：无（按场景）。
**次触发**：开始任何 bug 修复 / feature 实现；新增、更新或关闭 GitHub Issue；改 `docs/snapshot/specs/**` 设计稿。

> Bug / Feature 的完整执行流程见根 `AGENTS.md`「Bug / Feature 执行流程」。本分片仅定义 issue 操作的补充细节。

## 仓库与工具

- **仓库**：`rushsinging/aemeath`。
- **CLI**：`gh`（`gh auth status` 通过）。所有 issue 操作 **MUST** 用 `gh issue ... --repo rushsinging/aemeath`。
- **标题**：直接写问题描述（一句中文，80 字内），**NEVER** 加 `[Bug #N]` / `[Feature #N]` 前缀——`kind:*` label 已区分类型。

## 编号

- **NEVER** 手写 `Bug #N` / `Feature #N` 编号或前缀。GitHub 自动分配 issue 编号。
- 历史迁移条目的原 docs 编号作为标题前缀保留，body 顶部有 `<!-- Migrated from: <source> -->` 标记。

## Issue Body 规范

Body 建议结构（**SHOULD** 完整覆盖；轻量 issue 可裁剪）：
1. `## 现象` / `## 目标` —— bug 复现条件或 feature 要达成的效果。
2. `## 根因` / `## 设计` —— bug 根因或 feature 关键设计决策。
3. `## 修复 / 实现` —— 方案要点。
4. `## 验证` —— 复现命令、测试方法。
5. `## 涉及路径` —— 文件 / 模块路径。

## 状态流转

用 `gh issue edit` 更新 body 或 comment 表达：
- bug：`活动中` → `修复中` → `待确认` → 用户确认后关闭。
- feature：`计划中` → `实现中` → `待 review` → 合并后关闭。

## 设计稿联动

feature 类 issue **SHOULD** 配套 `docs/snapshot/specs/<file>.md` 设计稿。每份 spec 顶部已写 `> 对应 Issue: <url>`；修改 spec 时 **MUST** 同步更新该指针。

## 不属于本分片

- 改 `docs/snapshot/specs/**` 之外的 `docs/**`：按内容落到对应分片。
- 改 `specs/**` 自身：按改动内容分片。
