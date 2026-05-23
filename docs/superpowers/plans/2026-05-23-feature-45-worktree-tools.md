# Feature 45: EnterWorktree / ExitWorktree 工具 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 EnterWorktree/ExitWorktree 两个工具，让 LLM 显式切换 git worktree 上下文，更新 cwd/path_base/working_root，支持栈式 push/pop。

**Architecture:** ToolContext 新增 `context_stack: Arc<Mutex<Vec<WorkingContext>>>`，EnterWorktree 将当前上下文 push 到栈中然后切换，ExitWorktree pop 栈恢复。工具定义在 `packages/tools/src/worktree.rs`，注册到 `register_all_tools`。

**Tech Stack:** Rust, async_trait, serde_json, std::process::Command (git 校验)

---

### Task 1: ToolContext 新增上下文栈

**Files:**
- Modify: `packages/core/src/tool.rs` (新增 `WorkingContext` 结构体 + `context_stack` 字段 + `push_context`/`pop_context`/`enter_worktree`/`exit_worktree` 方法)
- Test: `packages/core/src/tool.rs` (同文件 `#[cfg(test)]` 块)

- [ ] **Step 1: 新增 `WorkingContext` 和 `context_stack` 字段**

在 `packages/core/src/tool.rs` 的 `ToolContext` struct 前新增：

```rust
/// 保存进入 worktree 前的工作上下文快照
#[derive(Debug, Clone)]
pub struct WorkingContext {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}
```

在 `ToolContext` struct 中新增字段：

```rust
    /// 上下文栈：EnterWorktree push，ExitWorktree pop
    pub context_stack: Arc<Mutex<Vec<WorkingContext>>>,
```

- [ ] **Step 2: 新增 `enter_worktree` / `exit_worktree` 方法**

在 `impl ToolContext` 中新增：

```rust
    /// 进入指定 worktree：push 当前上下文，然后切换 path_base/working_root
    pub fn enter_worktree(&self, path: PathBuf) -> Result<WorkingContext, String> {
        let path = if !path.is_absolute() {
            self.current_path_base().join(path)
        } else {
            path
        };

        // 校验路径存在且是 git worktree
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("路径不存在或无法访问 {}: {}", path.display(), e))?;

        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(&canonical)
            .output()
            .map_err(|e| format!("git rev-parse 执行失败: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "路径 {} 不是 git 仓库或 worktree",
                canonical.display()
            ));
        }

        let worktree_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let worktree_root = PathBuf::from(&worktree_root);

        // 校验是否与当前 repo 同源（同一 .git 目录）
        let current_root = self.current_working_root();
        if let Ok(common_git) = is_same_git_repo(&current_root, &worktree_root) {
            if !common_git {
                return Err(format!(
                    "路径 {} 不属于当前仓库（当前仓库根: {}）",
                    worktree_root.display(),
                    current_root.display()
                ));
            }
        }

        // 保存当前上下文
        let snapshot = WorkingContext {
            path_base: self.current_path_base(),
            working_root: self.current_working_root(),
        };
        self.context_stack
            .lock()
            .map(|mut s| s.push(snapshot.clone()))
            .unwrap_or_else(|e| e.into_inner().push(snapshot.clone()));

        // 切换到新 worktree
        self.set_working_directory(canonical);

        Ok(snapshot)
    }

    /// 退出当前 worktree：pop 栈恢复之前的上下文
    pub fn exit_worktree(&self) -> Result<WorkingContext, String> {
        let mut stack = self
            .context_stack
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        match stack.pop() {
            Some(prev) => {
                match self.working_root.lock() {
                    Ok(mut wr) => *wr = prev.working_root.clone(),
                    Err(poisoned) => *poisoned.into_inner() = prev.working_root.clone(),
                }
                match self.path_base.lock() {
                    Ok(mut pb) => *pb = prev.path_base.clone(),
                    Err(poisoned) => *poisoned.into_inner() = prev.path_base.clone(),
                }
                Ok(prev)
            }
            None => Err("上下文栈为空，没有可恢复的 worktree。可能已经在主工作区。".to_string()),
        }
    }
```

新增同文件级辅助函数：

```rust
/// 检查两个路径是否属于同一 git 仓库（通过 git rev-parse --git-dir 比对）
fn is_same_git_repo(a: &std::path::Path, b: &std::path::Path) -> Result<bool, String> {
    let git_dir_a = get_git_dir(a)?;
    let git_dir_b = get_git_dir(b)?;
    Ok(git_dir_a == git_dir_b)
}

fn get_git_dir(path: &std::path::Path) -> Result<PathBuf, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("git rev-parse --git-dir 执行失败: {}", e))?;

    if !output.status.success() {
        return Err("无法获取 git dir".to_string());
    }

    let git_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // gitdir 可能是相对路径，需要基于 worktree 根解析
    let git_dir = PathBuf::from(&git_dir_str);
    if git_dir.is_absolute() {
        Ok(git_dir)
    } else {
        Ok(path.join(&git_dir_str)
            .canonicalize()
            .unwrap_or_else(|_| path.join(&git_dir_str)))
    }
}
```

- [ ] **Step 3: 更新 ToolContext 所有构造点，添加 context_stack 字段**

搜索所有 `ToolContext {` 构造，添加 `context_stack: Arc::new(Mutex::new(Vec::new())),`。

主要位置：
- `packages/tools/src/bash.rs` 的测试
- `cli/src/tui/app/` 下所有构造 ToolContext 的地方

- [ ] **Step 4: 新增测试**

在 `packages/core/src/tool.rs` 的 `#[cfg(test)] mod tests` 中添加：

```rust
    #[test]
    fn test_context_stack_push_pop() {
        let (cwd, working_root, path_base) =
            ToolContext::new_working_paths(PathBuf::from("/tmp/test"));
        let ctx = ToolContext {
            cwd: cwd.clone(),
            working_root,
            path_base,
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: crate::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            context_stack: Arc::new(Mutex::new(Vec::new())),
        };

        // exit_worktree on empty stack should error
        assert!(ctx.exit_worktree().is_err());

        // 模拟 push/pop
        ctx.context_stack
            .lock()
            .unwrap()
            .push(WorkingContext {
                path_base: PathBuf::from("/tmp/prev"),
                working_root: PathBuf::from("/tmp/prev"),
            });
        let result = ctx.exit_worktree().unwrap();
        assert_eq!(result.path_base, PathBuf::from("/tmp/prev"));
        assert_eq!(ctx.current_path_base(), PathBuf::from("/tmp/prev"));
    }

    #[test]
    fn test_enter_worktree_rejects_nonexistent_path() {
        let (cwd, working_root, path_base) =
            ToolContext::new_working_paths(PathBuf::from("/tmp/test"));
        let ctx = ToolContext {
            cwd,
            working_root,
            path_base,
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: crate::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            context_stack: Arc::new(Mutex::new(Vec::new())),
        };
        assert!(ctx.enter_worktree(PathBuf::from("/nonexistent/path")).is_err());
    }
```

- [ ] **Step 5: 编译测试**

Run: `cargo test -p aemeath-core`
Expected: PASS（注意 `enter_worktree` 涉及 git 命令，测试仅验证非 git 路径拒绝和纯栈操作）

- [ ] **Step 6: Commit**

```bash
git add packages/core/src/tool.rs
git commit -m "feat: ToolContext 新增 context_stack + enter_worktree/exit_worktree 方法 (refs #45)"
```

---

### Task 2: 新增 EnterWorktree / ExitWorktree 工具

**Files:**
- Create: `packages/tools/src/worktree.rs`
- Modify: `packages/tools/src/lib.rs` (注册新工具)
- Test: `packages/tools/src/worktree.rs` (同文件测试块)

- [ ] **Step 1: 创建 `worktree.rs`**

```rust
//! EnterWorktree / ExitWorktree 工具
//!
//! 让 LLM 以结构化方式切换 git worktree 上下文，
//! 而不依赖 Bash cd 隐式切换。

use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

// ── EnterWorktree ──────────────────────────────────────────

pub struct EnterWorktreeTool;

#[derive(Debug, Deserialize)]
struct EnterInput {
    /// 必填：worktree 根目录路径（绝对或相对于当前 path_base）
    path: String,
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &'static str {
        "EnterWorktree"
    }

    fn description(&self) -> &str {
        "进入指定的 git worktree 目录，将工作上下文（cwd/path_base/working_root）切换到该 worktree。\n\
         后续所有相对路径操作（Read/Edit/Write/Glob/Grep/Bash）都将在该 worktree 中执行。\n\
         完成后使用 ExitWorktree 恢复原工作区。\n\n\
         **使用场景**：需要在独立 worktree 中修改代码时，优先使用本工具而非 Bash cd。\n\n\
         # 参数\n\
         - path（必填）：worktree 根目录路径，支持绝对路径或相对于当前工作目录的路径\n\n\
         # 校验\n\
         - 路径必须存在且是 git worktree\n\
         - worktree 必须属于当前仓库\n\n\
         # 输出\n\
         - 成功时显示当前工作根、git branch、repo root"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "worktree 根目录路径（绝对或相对路径）"
                }
            },
            "required": ["path"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let args: EnterInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(format!("参数错误: {e}")),
        };

        match ctx.enter_worktree(PathBuf::from(&args.path)) {
            Ok(_prev) => {
                let current = ctx.current_path_base();
                let root = ctx.current_working_root();
                let branch = get_current_branch(&current);
                ToolResult::success(format!(
                    "已进入 worktree: {}\n工作根: {}\ngit branch: {}\nrepo root: {}",
                    current.display(),
                    current.display(),
                    branch,
                    root.display(),
                ))
            }
            Err(e) => ToolResult::error(format!("进入 worktree 失败: {e}")),
        }
    }
}

// ── ExitWorktree ───────────────────────────────────────────

pub struct ExitWorktreeTool;

#[derive(Debug, Deserialize)]
struct ExitInput {
    /// 可选：直接切回指定路径，而不是 pop 栈
    #[serde(default)]
    pub path: Option<String>,
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &'static str {
        "ExitWorktree"
    }

    fn description(&self) -> &str {
        "退出当前 worktree，恢复到进入前的工作区上下文。\n\
         支持栈式操作：多次 EnterWorktree 后，每次 ExitWorktree 恢复上一层。\n\n\
         **使用场景**：worktree 中的修改/验证/提交完成后，调用本工具切回原工作区。\n\n\
         # 参数\n\
         - path（可选）：直接切回指定路径，忽略上下文栈\n\n\
         # 输出\n\
         - 成功时显示恢复后的工作根、git branch"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "可选：直接切回指定路径，忽略上下文栈"
                }
            },
            "required": []
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let args: ExitInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(format!("参数错误: {e}")),
        };

        if let Some(path) = args.path {
            // 直接切到指定路径（清空栈）
            match ctx.enter_worktree(PathBuf::from(&path)) {
                Ok(_) => {
                    // 进入新路径后把刚 push 的条目移除（这是"直接切到"而非"进入"）
                    ctx.context_stack
                        .lock()
                        .map(|mut s| s.pop())
                        .unwrap_or_else(|e| e.into_inner().pop());
                    let current = ctx.current_path_base();
                    let branch = get_current_branch(&current);
                    ToolResult::success(format!(
                        "已切换到: {}\ngit branch: {}",
                        current.display(),
                        branch,
                    ))
                }
                Err(e) => ToolResult::error(format!("切换失败: {e}")),
            }
        } else {
            match ctx.exit_worktree() {
                Ok(prev) => {
                    let current = ctx.current_path_base();
                    let branch = get_current_branch(&current);
                    ToolResult::success(format!(
                        "已退出 worktree，恢复到: {}\ngit branch: {}",
                        current.display(),
                        branch,
                    ))
                }
                Err(e) => ToolResult::error(format!("退出 worktree 失败: {e}")),
            }
        }
    }
}

// ── 辅助函数 ───────────────────────────────────────────────

fn get_current_branch(dir: &std::path::Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "(unknown)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_worktree_tool_schema() {
        let tool = EnterWorktreeTool;
        assert_eq!(tool.name(), "EnterWorktree");
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"].as_array().unwrap().iter().any(|r| r == "path"));
    }

    #[test]
    fn test_exit_worktree_tool_schema() {
        let tool = ExitWorktreeTool;
        assert_eq!(tool.name(), "ExitWorktree");
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        // path 是可选的
        assert!(schema["required"].as_array().unwrap().is_empty());
    }
}
```

- [ ] **Step 2: 注册工具**

在 `packages/tools/src/lib.rs` 中：

1. 新增 `pub mod worktree;`
2. 在 `register_all_tools` 中添加：
   ```rust
   registry.register(Box::new(worktree::EnterWorktreeTool));
   registry.register(Box::new(worktree::ExitWorktreeTool));
   ```
3. 在 `register_all_tools_except_agent` 中添加同样的两行。
4. `register_subagent_tools` 中 **不添加**（子代理不应自行切换 worktree）。

- [ ] **Step 3: 编译测试**

Run: `cargo test -p aemeath-tools`
Expected: PASS

- [ ] **Step 4: 更新 bash.rs 测试中的 ToolContext 构造**

在 `packages/tools/src/bash.rs` 的测试中添加 `context_stack: Arc::new(Mutex::new(Vec::new())),`。

Run: `cargo test -p aemeath-tools`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add packages/tools/src/worktree.rs packages/tools/src/lib.rs packages/tools/src/bash.rs
git commit -m "feat: 新增 EnterWorktree/ExitWorktree 工具 (refs #45)"
```

---

### Task 3: 更新 CLI 层 ToolContext 构造点

**Files:**
- Modify: `cli/src/tui/app/` 下所有 `ToolContext {` 构造，添加 `context_stack` 字段

- [ ] **Step 1: 搜索所有构造点**

Run: `grep -rn "ToolContext {" cli/src/`
逐一添加 `context_stack: Arc::new(Mutex::new(Vec::new())),`

- [ ] **Step 2: 编译验证**

Run: `cargo build -p aemeath-cli`
Expected: 编译通过

- [ ] **Step 3: 运行全量测试**

Run: `cargo test -p aemeath-cli`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add cli/src/
git commit -m "feat: CLI 层 ToolContext 添加 context_stack 字段 (refs #45)"
```

---

### Task 4: 更新 sub-agent 工具注册排除列表测试

**Files:**
- Modify: `packages/tools/src/lib.rs` (测试)

- [ ] **Step 1: 更新 sub-agent 排除列表测试**

在 `packages/tools/src/lib.rs` 的 `test_register_subagent_tools_excludes_coordination_tools` 测试中，向 `forbidden` 数组添加 `"EnterWorktree"` 和 `"ExitWorktree"`：

```rust
          for forbidden in [
              "Agent",
              "AskUserQuestion",
              "EnterWorktree",
              "ExitWorktree",
              // ... 其余不变
          ] {
```

- [ ] **Step 2: 运行测试**

Run: `cargo test -p aemeath-tools test_register_subagent_tools`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add packages/tools/src/lib.rs
git commit -m "test: sub-agent 排除 EnterWorktree/ExitWorktree (refs #45)"
```

---

### Task 5: 更新文档

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: 更新 feature 45 状态**

将 active.md 中 feature 45 的状态从 "待实施" 改为 "已完成"，并补充实现摘要。

- [ ] **Step 2: Commit**

```bash
git add docs/feature/active.md
git commit -m "docs: feature #45 状态更新为已完成"
```

---

### Task 6: 合并、验证、清理

- [ ] **Step 1: 合并到 main**

```bash
cd /path/to/main
git merge feature/45-worktree-tools --no-edit
```

- [ ] **Step 2: 在 main 上运行全量验证**

```bash
cargo build && cargo test -p aemeath-core && cargo test -p aemeath-tools && cargo test -p aemeath-cli
```

- [ ] **Step 3: 清理 worktree**

```bash
git worktree remove .worktrees/<name>
```
