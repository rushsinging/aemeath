# Bug / Feature 追踪联动

**Scope**：无路径触发。任何 bug 修复或 feature 实现的流程约束；改 `docs/bug/**` 或 `docs/feature/**` 时适用。
**主触发**：无（按场景）。
**次触发**：开始任何 bug 修复 / feature 实现；新增、更新或归档 `docs/bug/**`、`docs/feature/**` 条目。

## 开工前

- **MUST** 开始工作前查看 `docs/bug/active.md` 和 `docs/feature/active.md`，确认当前修改是否与已知条目相关。

## 编号与查找

- **编号独立**：bug 与 feature **NEVER** 共享编号序列，各自独立递增。bug 编号取 `docs/bug/active.md` 与 `docs/bug/archived/` 的最大值 +1；feature 编号取 `docs/feature/active.md` 与 `docs/feature/archived/` 的最大值 +1。新增条目前 **MUST** 在对应类别内核对最大编号，不得跨类别取号。
- **查找固定文档**：查询 bug / feature 时，**MUST** 优先查找固定追踪文档：活跃 bug 查 `docs/bug/active.md`，活跃 feature 查 `docs/feature/active.md`；归档条目查 `docs/bug/archived/` 或 `docs/feature/archived/`。按编号查找时 **MUST** 在对应 `active.md` 中搜索编号标题（如 `#70`）并阅读命中行附近的详细章节，NEVER 只根据顶部表格摘要下结论。

## 修复流程

- **Bug 修复 / feature 实现 MUST 使用 git worktree**（详见根 `AGENTS.md` 工作流约束）。
- **MUST** 修复 bug 时先添加重现该 bug 的测试用例，再提交修复代码。
- **Bug 状态流程**：`活动中` → `修复中` → `待确认` → 用户确认后归档。
- **修改涉及已知 bug 时 MUST**：
  1. 在 `docs/bug/active.md` 的对应行更新状态。
  2. 在 commit message 中引用 bug 编号（如 `refs #1`）。
  3. 修复后将 commit hash 更新到归档文件的"修复"字段。
- **新增 bug 发现时 MUST**：在 `docs/bug/active.md` 表格中添加行（状态"活动中"），并在详情区域记录症状、根因、修复方向。
- **实现 feature 时 MUST**：在 `docs/feature/active.md` 登记，完成后归档。
- **MUST** 解决 bug 或完成 feature 后，同步更新 `docs/bug/active.md` 或 `docs/feature/active.md`，记录问题、解决思路和当前解决状态。

## 归档门禁

- bug 修复或 feature 完成后，**MUST** 等待用户确认，确认后从 `active.md` 移除并将详情总结到 `archived/`。在 `main` 上更新文档后 **MUST** 立即提交，不与其他改动混入同一 commit。
