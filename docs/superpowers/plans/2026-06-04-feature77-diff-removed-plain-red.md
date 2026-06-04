# Plan: Feature #77 diff removed 行纯红显示

## 前置

- 当前 worktree：`feature/77-diff-removed-plain-red`。
- 已加载适用规范：`specs/bug-feature-tracking.md`、`specs/rust-coding.md`、`specs/tui-cli.md`。
- 设计采用备选方案 2：新增 deleted/removed 专用 helper。

## 步骤

1. 更新 unified diff 测试
   - 修改 `test_render_unified_diff_removed_and_context_lines_use_syntax_highlight`：removed 行不再期待高亮。
   - 新增/调整断言：removed 行正文 span 使用 `theme::DIFF_REMOVE_FG`，且不产生语法高亮多色 span。
   - 保持 added/context 高亮断言。
   - 修改 `test_render_unified_diff_infers_extension_from_file_headers`：只要求 added 行因推断扩展名高亮，removed 行纯红。

2. 更新普通 diff 测试
   - 在 `apps/cli/src/tui/render/output/diff.rs` 测试中增加 delete 行纯红断言。
   - 覆盖 `Some("rs")` 场景，确保 delete 行不会因为 syntax_ref 产生非 `DIFF_REMOVE_FG` 的正文颜色。

3. 实现 unified diff removed 专用 helper
   - 在 `unified_diff.rs` 新增 `push_removed_body(parts, body)`。
   - `DiffLineKind::Removed` arm 改为调用该 helper。
   - helper 只追加纯 `theme::DIFF_REMOVE_FG` span。

4. 实现普通 diff delete 专用 helper
   - 在 `diff.rs` 新增 `push_deleted_text(spans, text)`。
   - `build_delete_line()` 移除 `syntax_ref` 参数或保留但不使用；优先移除参数以避免误用。
   - `ChangeTag::Delete` 调用同步更新。

5. 同步 feature 追踪文档
   - 将 `docs/feature/active.md` 中 #77 状态更新为“修复中”。
   - 在详情区补充实现和验证结果；如无详情区则新增 `### #77 ...` 小节。

6. 验证
   - `cargo test -p cli diff`
   - `cargo test -p cli unified_diff`
   - `cargo fmt -p cli --check`
   - `git diff --check`
   - 如影响渲染更广，追加 `cargo test -p cli render`。

7. 提交与合并
   - 按仓库 commit 风格提交，message 引用 `refs #77`。
   - 退出 worktree，main 拉取最新，merge 回 main。
   - main 上重跑关键验证后清理 worktree。
