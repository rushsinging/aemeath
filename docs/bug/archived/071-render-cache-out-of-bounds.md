# Bug #71：TUI 渲染缓存越界 panic（len 10000 / index 10000）+ unsafe string guard 覆盖不全

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染缓存端点滞留 + 架构 guard 覆盖不全 |

## 症状

长会话中 TUI 直接崩溃：

```text
[PANIC] index out of bounds: the len is 10000 but the index is 10000
       at apps/cli/src/tui/output_area/rendered_lines.rs:98:21
```

`rendered_lines.rs:98` 在 `collect_table_ranges` 外层循环 `while i < end { let line = &lines[i]; ... }`。`lines.len() == 10000`（等于 `MAX_LINES`），`i == 10000` → `end > lines.len()`。

## 影响

输出区累计行数达到上限（`output_area/types.rs: MAX_LINES = 10000`）后，任意触发渲染（滚动、流式追加、resize）都可能 panic，整个 TUI 进程崩溃退出。崩溃发生在正常 agent loop 收尾之前，**Stop hook（架构守卫 / 单测 / build）也来不及执行**，表现为"stop hook 没有生效"。

## 根因

1. 输出区内容是上限 10000 行的 `VecDeque`（`content.rs`：超过 `MAX_LINES` 时从头部 pop）。
2. `RenderedLineCache`（`rendered_cache.rs`）以行下标缓存渲染结果，并维护 `render_start` / `render_end` 渲染区间。
3. `ensure_rendered` 的 dirty 分支用 `block_start` / `block_end`（均 ≤ `total`）调用 `render_range`，安全；但**增量分支**直接用 `self.render_start` / `self.render_end` 作为 `render_range` 区间端点。当 `lines` 长度因到达上限或裁剪发生变化、而 `render_start` / `render_end` 仍是旧值（> 当前 `total`）时，`render_range` 收到 `end > lines.len()`，在 `collect_table_ranges` 处越界。
4. `content_changed` 只 `truncate` 了 `cache`，没有同步 clamp `render_start` / `render_end`。

## 修复

### 渲染缓存层（结构性消除）

- #58 新渲染管线已移除旧 `rendered_lines` / `render_range` 行下标缓存路径，原越界路径整体消失。
- 在新管线中补充 MAX_LINES 裁剪后缓存 retain 回归 `test_render_tree_retains_only_trimmed_live_blocks`，确保超过 `MAX_LINES` 后只保留裁剪后的 block 缓存，旧 block 不再滞留。
- TUI 中相关裸下标（`src[idx]` / `screen_line_map[rel_row]`）改为 `.get()` 防御访问。

### Guard 层（覆盖范围扩展）

- 扩展 `check-unsafe-text-ops.sh` 对带显式 allow 的裸单下标进行拦截，当前架构 guard 报告 unsafe TUI text/index operations 为 **0**。
- 已识别但本次未在 TUI 之外强制的缺口（后续若再发同源问题，按需再扩）：
  - 扫描范围仍限 `apps/cli/src/tui`，`agent/` 与 `packages/` 的原始字节/字符切片不被检查（已有 `agent/share/src/string_idx::CharIdx`，但未强制使用）。
  - 仅 Stop hook 触发，非每次编辑/PreToolUse/PostToolUse 触发；本类 panic 异常结束时整体跳过 guard。

## 相关提交

- `4b34a6f` fix(tui): 防止渲染裁剪后缓存越界 (refs #71)
- #58 渲染管线重构系列

## 涉及路径

- `apps/cli/src/tui/output_area/rendered_lines.rs`（旧 `render_range`/`collect_table_ranges` 越界点，已随 #58 移除）
- `apps/cli/src/tui/output_area/rendered_cache.rs`（旧 `ensure_rendered`/`content_changed`，已随 #58 移除）
- `apps/cli/src/tui/output_area/content.rs`、`types.rs`（`MAX_LINES` 裁剪逻辑）
- `.agents/hooks/check-unsafe-text-ops.sh`（裸单下标拦截扩展）

## 验证

2026-05-30 用户确认 bug #71 已修复。
