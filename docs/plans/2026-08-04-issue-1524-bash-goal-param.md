# Bash tool 增加 goal 必填参数，命令全文移至 TUI details

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Issue:** #1524
**Milestone:** v0.1.0 — Context Engineering + 架构重构

**Goal:** Bash tool 新增必填参数 `goal`（命令目标/意图简述），作为 TUI header 显示文本；命令全文移到 details 区域，不截断、自动换行。空 `goal` 时 fallback 到旧行为（header 截断显示 command），保证历史 session 反序列化兼容。

**Architecture:** 跨层 4 层改动：

- **Domain types**（`agent/features/tools/src/domain/types/bash.rs`）：`BashInput` 新增 `goal: String`（非 Option，build.rs 自动标记 required），排在 `command` 前。`call()` 仍执行 `command`，`is_input_safe` 仍检查 `command`，不受影响。
- **i18n**（`agent/shared/src/i18n/tools/filesystem.rs`）：bash description 文案补充 `goal` 参数说明。
- **TUI display**（`apps/cli/src/tui/render/output/tool_display/tool_impls/shell.rs`）：`BashDisplay` 的 `format_header` / `format_header_line_with_result` 用 `goal` 作 header；`format_details` 返回命令全文（Vec 单元素）；空 `goal` fallback 到旧的截断行为。
- **附带**（`AGENTS.md`）：触发表路径修正（`specs/xxx.md` → `specs/3.N-xxx.md`），与磁盘实际文件名对齐。

**Tech Stack:** Rust，无新依赖。

**向后兼容：** `BashInput` 已有 `#[serde(default)]`，老 session JSON 缺少 `goal` 字段时反序列化为空串，TUI fallback 到旧行为。

---

## 文件结构

- Modify: `agent/features/tools/src/domain/types/bash.rs` —— `BashInput` 新增 `goal` 字段
- Modify: `agent/shared/src/i18n/tools/filesystem.rs` —— bash description 补充 goal 说明
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/shell.rs` —— header 用 goal，details 返回命令全文
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/shell.rs` 内或同模块测试 —— L1 渲染测试
- Modify: `agent/features/tools/src/adapters/bash/tests.rs` —— BashInput 反序列化 + schema 测试
- Modify: `AGENTS.md` —— 触发表路径修正
- Modify: `specs/3.3-tui-cli.md` —— Bash render_policy 配置表更新（details 从 Expanded + 空变为 Expanded + 命令全文）

不改：`call()` 执行逻辑、`is_input_safe`、result 渲染策略（`tail_mode: true, max_lines: 5`）、`BashResult` 结构。

---

### Task 1: Domain types — BashInput 新增 goal 字段（TDD 红）

**Files:**
- Modify: `agent/features/tools/src/domain/types/bash.rs`
- Modify: `agent/features/tools/src/adapters/bash/tests.rs`（或 domain/types 同模块测试）

- [ ] **Step 1: 写失败测试**

在 `agent/features/tools/src/adapters/bash/tests.rs` 中新增（或修改已有）测试：

```rust
#[test]
fn bash_input_goal_required_and_deserialization() {
    // 1. goal 为空时反序列化仍成功（serde default 兼容老 session）
    let old_json = serde_json::json!({"command": "ls -la", "timeout": 5000});
    let input: BashInput = serde_json::from_value(old_json).unwrap();
    assert_eq!(input.command, "ls -la");
    assert!(input.goal.is_empty(), "老 session 缺 goal 应反序列化为空串");

    // 2. goal 字段正常解析
    let new_json = serde_json::json!({"goal": "列出文件", "command": "ls -la"});
    let input: BashInput = serde_json::from_value(new_json).unwrap();
    assert_eq!(input.goal, "列出文件");
    assert_eq!(input.command, "ls -la");

    // 3. schema 中 goal 在 required 列表
    let schema = BashInput::data_schema();
    let required = schema.get("required").unwrap();
    let required_str = required.to_string();
    assert!(required_str.contains("\"goal\""), "goal 应在 required 中");
    assert!(required_str.contains("\"command\""), "command 应在 required 中");
}
```

- [ ] **Step 2: 运行测试确认红**

```bash
cargo test -p tools bash_input_goal -- --nocapture
```

- [ ] **Step 3: 添加 goal 字段**

在 `agent/features/tools/src/domain/types/bash.rs` 的 `BashInput` struct 中，在 `command` 前新增：

```rust
/// A short description of the command goal/intent, shown as the TUI header
pub goal: String,
```

由于 `goal` 是 `String`（非 `Option`），build.rs 会自动将其加入 `required` 列表。`#[serde(default)]` 在 struct 级别已存在，缺失字段反序列化为空串。

- [ ] **Step 4: 运行测试确认绿**

```bash
cargo test -p tools bash_input_goal -- --nocapture
```

---

### Task 2: i18n — bash description 补充 goal 参数说明

**Files:**
- Modify: `agent/shared/src/i18n/tools/filesystem.rs`

- [ ] **Step 1: 更新文案**

将 `bash()` 函数返回值补充 `goal` 参数说明。英文版：

```text
"Executes a bash command and returns its output. The `goal` parameter (required) is a short description of the command intent, shown in the TUI header. Working directory persists between calls but shell state does not. Chain commands with &&. Optional timeout parameter (default 120s, max 600s)."
```

中文版同步补充。

- [ ] **Step 2: 更新现有 i18n 测试**

`filesystem_bilingual_and_fallback` 测试断言中补充对 `goal` 关键词的检查（如英文含 "goal"、中文含"目标"）。

- [ ] **Step 3: 运行测试**

```bash
cargo test -p aemeath-shared filesystem_bilingual -- --nocapture
```

---

### Task 3: TUI display — header 用 goal，details 显示命令全文（TDD 红）

**Files:**
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls/shell.rs`

- [ ] **Step 1: 写失败测试**

在 `shell.rs` 末尾新增 `#[cfg(test)] mod tests` 模块（如不存在），测试覆盖三种场景：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shows_goal_when_present() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "运行测试",
            "command": "cargo test -- --nocapture"
        });
        let header = display.format_header(&input, None);
        assert!(header.contains("运行测试"));
        // header 不含命令全文
        assert!(!header.contains("cargo test"));
    }

    #[test]
    fn header_falls_back_to_command_when_goal_empty() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "",
            "command": "cargo build"
        });
        let header = display.format_header(&input, None);
        // goal 为空时 fallback：header 显示截断的 command
        assert!(header.contains("cargo build"));
    }

    #[test]
    fn details_show_full_command_untruncated() {
        let display = BashDisplay;
        let long_command = "echo hello world && ".repeat(20);
        let input = serde_json::json!({
            "goal": "测试长命令",
            "command": long_command
        });
        let details = display.format_details(&input);
        assert_eq!(details.len(), 1);
        // 命令全文在 details 中，不截断
        assert_eq!(details[0], long_command);
    }

    #[test]
    fn format_header_line_with_result_uses_goal() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "构建项目",
            "command": "cargo build --release"
        });
        let line = display.format_header_line_with_result(&input, None, None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("构建项目"));
        assert!(!text.contains("cargo build --release"));
    }
}
```

- [ ] **Step 2: 运行测试确认红**

```bash
cargo test -p cli header_shows_goal_when_present -- --nocapture
```

- [ ] **Step 3: 改 format_header 用 goal**

将 `format_header` 改为：

```rust
fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
    let args = parse_input::<BashInput>(input);
    if !args.goal.is_empty() {
        format!("{} {}", self.display_name(), args.goal)
    } else if !args.command.is_empty() {
        // fallback：老 session 无 goal，显示截断的 command
        format!("{} {}", self.display_name(), truncate_ellipsis(&args.command, 80))
    } else {
        self.display_name().to_string()
    }
}
```

- [ ] **Step 4: 改 format_details 返回命令全文**

将 `format_details` 从返回空 Vec 改为返回命令全文：

```rust
fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
    let args = parse_input::<BashInput>(input);
    if args.command.is_empty() {
        Vec::new()
    } else {
        vec![args.command]
    }
}
```

- [ ] **Step 5: 改 format_header_line_with_result 用 goal**

将 `format_header_line_with_result` 的 header 部分改为使用 goal（与 format_header 逻辑对齐），suffix 逻辑不变。

- [ ] **Step 6: 运行测试确认绿**

```bash
cargo test -p cli header_shows_goal -- --nocapture
cargo test -p cli header_falls_back -- --nocapture
cargo test -p cli details_show_full -- --nocapture
cargo test -p cli format_header_line_with_result_uses_goal -- --nocapture
```

---

### Task 4: specs 文档更新

**Files:**
- Modify: `specs/3.3-tui-cli.md`
- Modify: `AGENTS.md`（触发表路径修正）

- [ ] **Step 1: 更新 specs/3.3-tui-cli.md**

在 3.3.4.6 配置表中，Bash 行的 Details 列从 `Expanded` 保持不变，但补充注释说明 details 现在包含命令全文（非空）。

在 3.3.4.7 关键设计决策中补充：
- **Bash 的 `format_details` 返回命令全文**（不截断），由渲染层 Word wrap 自动换行。header 显示 `goal` 参数（必填）；空 `goal` fallback 到截断的 command。

- [ ] **Step 2: 修正 AGENTS.md 触发表**

将所有 `specs/xxx.md` 引用修正为实际文件名 `specs/3.N-xxx.md`，与磁盘对齐。

- [ ] **Step 3: 修正 AGENTS.md §2 中引用**

§2 工作流段落、§2.1 运行时路径中的 specs 引用同样补齐数字前缀。

---

### Task 5: 全量验证

- [ ] **Step 1: cargo fmt**

```bash
cargo fmt
```

- [ ] **Step 2: cargo clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: cargo test**

```bash
cargo test --workspace
```

- [ ] **Step 4: 架构守卫**

```bash
bash .agents/hooks/check-architecture-guards.sh
```

- [ ] **Step 5: 手动 TUI 验证**

```bash
cargo build -p cli && AEMEATH_LOG_LEVEL=debug cargo run
```

在 TUI 中触发 Bash 工具调用，确认：
- header 显示 `Bash <goal>`
- details 区域显示命令全文（不截断、自动换行）
- result 子块仍只显示最后 5 行输出

---

## 风险与回退

- **风险**：LLM 可能不填 `goal`（虽然 schema required）。`call()` 不依赖 `goal`，仅 TUI 层消费，空值时 fallback，无功能风险。
- **回退**：如需回退，删除 `goal` 字段即可，`format_details` 恢复空 Vec。
