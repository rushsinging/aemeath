# #1420 前置根窗口选择第一刀实施计划

> **执行要求：** 在独立 worktree 中按 TDD 实施。本切片只消除 `MAX_LINES` 裁剪后的静态 Edit 重建循环，不宣称完成完整 viewport virtualization。

**目标：** 让已经测量过的历史 root 在后续 spinner/无关 revision 帧中，于 `render_node` 和 syntax highlight 之前完成窗口选择，避免旧 Edit 反复经历“重建 → 裁剪 → 缓存淘汰”。

**Issue：** [#1420](https://github.com/rushsinging/aemeath/issues/1420)，Parent [#1417](https://github.com/rushsinging/aemeath/issues/1417)，基线 [#1418](https://github.com/rushsinging/aemeath/issues/1418)。

## 范围

### 本切片包含

- 在 `OutputDocumentRenderer` 内保存轻量 root 布局索引：root 内容/宽度/间距/动画相关指纹与已渲染行数。
- 每帧先解析 root 行数：索引命中时不渲染；仅新 root 或指纹变化的 root 需要测量渲染。
- 根据 root 行数从最新端选择不超过 `MAX_LINES` 的完整 root groups，再组装最终文档。
- 保持最新单个超大 root 即使超过预算也完整保留。
- 内容与 gutted cache 仍只保留当前选中窗口；root 布局索引保留所有仍存在于语义树中的轻量行数元数据。
- 旧 root 被语义树删除、宽度/间距/内容/动画布局变化时，布局索引确定性失效。

### 非目标

- 不接入真实 viewport height、overscan 或 `history_window_tail_offset`；仍使用现有 `MAX_LINES` 上限。
- 不解决 resume 冷首帧必须测量未知 root 的成本。
- 不引入有界 LRU、超大 diff 高亮降级、选择/复制坐标重构。
- 不把 #1418 未合入的 cache miss 原因 collector 复制到本分支。

## 设计

### RootLayoutKey

为每棵 root 子树计算轻量稳定指纹，覆盖：

- `outer_width`；
- Markdown spacing；
- 每个 node 的 `block_id`、`block_version`、children 顺序；
- 会影响 gutter/placeholder 布局的 marker frame。

指纹只扫描 `BlockNode` 元数据，不读取或复制大文本，也不执行 Markdown、diff 或 syntax highlight。

### RootLayoutIndex

`OutputDocumentRenderer` 保存 `block_id -> (layout_key, line_count)`。每次 render：

1. 为全部 roots 计算 layout key。
2. key 命中时直接取得行数。
3. key 未命中时只渲染该 root 一次，记录 group 与行数，供当前帧直接复用。
4. 根据全部 root 行数从最新端选择窗口。
5. 只对已选且本帧未预渲染的 root 调用 `render_node`。
6. content/gutted cache retain 仅保留最终窗口；layout index retain 所有仍在语义树中的 roots。

首次冷渲染因所有 root 未知，仍会测量全部历史；后续帧以及追加无关新 root 时，旧静态 Edit 不再重建。

## TDD

### RED

在 `document_renderer/tests.rs` 构造超过 `MAX_LINES` 的多个静态 Edit root：

1. 首帧渲染并建立布局索引。
2. 第二帧推进 spinner frame，语义内容不变。
3. 断言第二帧 `edit_diff_calls == 0`、`diff_build_calls == 0`、`syntax_highlight_calls == 0`。

当前实现会重新渲染上一帧被裁剪并淘汰的旧 Edit，因此测试必须先失败。

再增加无关 revision 等价场景：clone ViewModel、只提高 `version` 并追加一个普通静态 root，旧 Edit 不重新高亮。

### GREEN

实现 root layout index 与前置选择，使上述测试通过，同时保持现有：

- parent/child group 不拆分；
- 最新超大 group 保留；
- width/spacing/child version 变化重排；
- cache retain 删除语义树中不存在的 block；
- 历史窗口场景测试。

## 验证

- `cargo fmt --all -- --check`
- `cargo test -p cli tui::render::output::document_renderer --no-fail-fast`
- `cargo test -p cli tui::app::scenario_tests::history_window --no-fail-fast`
- `cargo test -p cli tui::view_assembler::output::tests::edit_diff_performance --no-fail-fast`
- `cargo test -p cli --release edit_diff_release_workload -- --ignored --nocapture`
- `cargo test -p cli --no-fail-fast`
- `cargo check -p cli`
- `cargo clippy -p cli --all-targets -- -D warnings`
- `git diff --check`

## 后续切片

1. 将 App 的 `render_line_limit + tail_offset` 作为窗口请求传给 renderer，替代固定 `MAX_LINES`。
2. 建立 viewport height + overscan block/line 索引。
3. 为 root/layout/content cache 引入有界 LRU。
4. 对未知超大 Edit 首帧增加可见行预算与确定性纯色降级。
