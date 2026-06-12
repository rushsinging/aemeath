# scripts/migrate-docs-to-issues

将 `docs/bug/` 与 `docs/feature/` 下的所有条目（约 188 条）批量迁移到
GitHub Issues。本地 docs 目录迁完后删除，issue 成为唯一信息源。

## 使用

### 1. Dry-run：生成草稿（默认）

```bash
python3 scripts/migrate-docs-to-issues/migrate.py
```

输出：
- `scripts/migrate-docs-to-issues/draft/<kind>_<id>.md`：每条 issue body 草稿
- `scripts/migrate-docs-to-issues/draft/summary.json`：草稿清单（含 title/labels/source）
- `scripts/migrate-docs-to-issues/migration-map.json`：完整映射表（`issue_number` 字段在 apply 后填）

### 2. Apply：实际创建 issue

```bash
python3 scripts/migrate-docs-to-issues/migrate.py --apply
```

行为：
- 拉取 `rushsinging/aemeath` 远端所有 `migrated-from:docs` label 的 issue
- 提取其 body 顶部的 `<!-- Migrated from: <src> -->` 溯源注释
- 与待迁条目的 `source` 比对，已迁条目自动跳过（**幂等**）
- 其余条目调 `gh issue create` 批量创建
- 创建结果写回 `migration-map.json` 的 `issue_number` 字段

### 3. 调试参数

```bash
# 限制最多创建 5 条
python3 scripts/migrate-docs-to-issues/migrate.py --apply --limit 5

# 指定其他仓库（默认 rushsinging/aemeath）
python3 scripts/migrate-docs-to-issues/migrate.py --apply --repo owner/repo

# 同步映射：拉远端已迁 issue 的 source→issue_number 写回 migration-map.json
# 用于本地映射表丢失/漂移后修复（不创建新 issue）
python3 scripts/migrate-docs-to-issues/migrate.py --apply --sync-map
```

## 标签体系

每个 issue 携带：

- `kind:bug` / `kind:feature`：类型
- `priority:high` / `priority:medium` / `priority:low`：优先级（来自 docs 表格；空则不标）
- `migrated-from:docs`：溯源

## 幂等原理

- 创建时：issue body 顶部写 `<!-- Migrated from: <source> -->` 注释
- 重跑时：`gh issue list --label migrated-from:docs --json body` 拉取已迁条目的 source 集合
- 跳过：`source` 已在远端集合中的条目

`source` 格式：
- archived 文件：`docs/{bug,feature}/archived/<id>-<slug>.md`
- active 文件：`docs/{bug,feature}/active.md#<id>`（带 id 锚点，让 active.md 内多条独立溯源）

## 解析的 docs 结构

- `docs/{bug,feature}/active.md`：表格行（# / 标题 / 优先级 / 状态 / ...）+ `### #<id> <标题>` 详情段
- `docs/{bug,feature}/archive.md`：表格行（# / 标题 / archived 链接），仅作交叉验证
- `docs/{bug,feature}/archived/<id:03d>-<slug>.md`：单条详情，文件名 id 为权威

## 不在迁移范围

- `docs/feature/specs/*.md`（设计稿）：保留在原位，由后续脚本在每份顶部加「对应 Issue」链接
- `docs/feature/template.md`（归档模板）：保留

## 迁移后清理

apply 完成后：
1. 删除 `docs/bug/{active,archive}.md` 与 `docs/bug/archived/`
2. 删除 `docs/feature/{active,archive}.md` 与 `docs/feature/archived/`
3. `docs/feature/specs/` 保留，仅在每份 md 顶部追加「对应 Issue: #<num>」链接
4. 更新 `AGENTS.md` 工作流约束段与 `specs/bug-feature-tracking.md` 分片
