# Codex 风格配置迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Aemeath 的配置、指令和 skills 读取迁移到 `~/.agents` / `AGENTS.md` / `.agents/skills` 新模式，并在本次更新后对旧 `.aemeath` / `CLAUDE.md` / skills 做一次性复制迁移。

**Architecture:** 新增一个集中路径/迁移模块，统一计算全局与项目路径，避免配置、prompt、skills 各自拼路径。启动加载时先执行不覆盖的一次性迁移，再只读取新路径；迁移只复制不删除旧文件，失败记录 warning 且不阻塞启动。项目路径以启动 `cwd` 为准，worktree 中不会跨 checkout 读取主工作区，避免误共享。

**Tech Stack:** Rust、tokio fs、serde_json、现有 ConfigManager / HookRunner / skill loader、cargo test。

---

## File Structure

- Create: `packages/core/src/config/paths.rs`
  - 负责统一路径常量、`~/.agents` 根目录解析、项目/全局配置路径、AGENTS.md 路径、skills 路径、一次性迁移辅助函数。
- Modify: `packages/core/src/config/mod.rs`
  - 导出 `paths` 模块，更新配置层级注释与测试期望。
- Modify: `packages/core/src/config/manager/mod.rs`
  - `ConfigManager::new` 使用新路径；`load` 读取前迁移全局/项目 `config.json -> aemeath.json`。
- Modify: `packages/core/src/config/manager/persistence.rs`
  - 错误消息改中文，保存仍写新路径。
- Modify: `packages/core/src/config/skills.rs`
  - 更新注释。
- Modify: `packages/core/src/skill/loader.rs`
  - skills 读取前迁移旧目录，新读取顺序改为 `{cwd}/.agents/skills` 优先、`~/.agents/skills` 其次、extra dirs 最后。
- Modify: `apps/cli/src/prompt.rs`
  - `load_claude_md` 替换为 `load_agents_md`；读取前迁移 `CLAUDE.md -> AGENTS.md`；Hook source 改为 `agents_md`；安全扫描文件名改为 `AGENTS.md`。
- Modify: `docs/feature/active.md`
  - 将 #40 状态更新为“实施中”，记录一次性迁移策略。

---

### Task 1: 新增统一路径与迁移模块

**Files:**
- Create: `packages/core/src/config/paths.rs`
- Modify: `packages/core/src/config/mod.rs`

- [ ] **Step 1: 编写路径与迁移模块**

Create `packages/core/src/config/paths.rs`:

```rust
//! Codex 风格配置路径与一次性迁移辅助。

use std::path::{Path, PathBuf};

pub const AGENTS_DIR_ENV: &str = "AEMEATH_AGENTS_DIR";
pub const NEW_CONFIG_FILE: &str = "aemeath.json";
pub const OLD_CONFIG_FILE: &str = "config.json";
pub const AGENTS_MD: &str = "AGENTS.md";
pub const CLAUDE_MD: &str = "CLAUDE.md";
pub const AGENTS_DIR_NAME: &str = ".agents";
pub const OLD_AEMEATH_DIR_NAME: &str = ".aemeath";
pub const SKILLS_DIR_NAME: &str = "skills";

pub fn global_agents_dir() -> PathBuf {
    if let Ok(value) = std::env::var(AGENTS_DIR_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return expand_home(Path::new(trimmed));
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(AGENTS_DIR_NAME)
}

pub fn global_config_path() -> PathBuf {
    global_agents_dir().join(NEW_CONFIG_FILE)
}

pub fn old_global_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OLD_AEMEATH_DIR_NAME)
        .join(OLD_CONFIG_FILE)
}

pub fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(AGENTS_DIR_NAME)
        .join(NEW_CONFIG_FILE)
}

pub fn old_project_config_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(OLD_AEMEATH_DIR_NAME)
        .join(OLD_CONFIG_FILE)
}

pub fn global_agents_md_path() -> PathBuf {
    global_agents_dir().join(AGENTS_MD)
}

pub fn old_global_claude_md_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join(CLAUDE_MD)
}

pub fn project_agents_md_path(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_MD)
}

pub fn old_project_claude_md_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_MD)
}

pub fn global_skills_dir() -> PathBuf {
    global_agents_dir().join(SKILLS_DIR_NAME)
}

pub fn old_global_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(OLD_AEMEATH_DIR_NAME)
        .join(SKILLS_DIR_NAME)
}

pub fn project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn old_project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub async fn migrate_file_once(old_path: &Path, new_path: &Path) -> Result<bool, String> {
    if new_path.exists() || !old_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = new_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建迁移目标目录失败 {}: {e}", parent.display()))?;
    }

    tokio::fs::copy(old_path, new_path)
        .await
        .map_err(|e| format!("迁移文件失败 {} -> {}: {e}", old_path.display(), new_path.display()))?;

    Ok(true)
}

pub fn migrate_dir_once(old_path: &Path, new_path: &Path) -> Result<bool, String> {
    if new_path.exists() || !old_path.exists() {
        return Ok(false);
    }
    if !old_path.is_dir() {
        return Ok(false);
    }

    copy_dir_all(old_path, new_path)
        .map_err(|e| format!("迁移目录失败 {} -> {}: {e}", old_path.display(), new_path.display()))?;
    Ok(true)
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_project_paths_use_agents_directory() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(project_config_path(&cwd), PathBuf::from("/tmp/demo/.agents/aemeath.json"));
        assert_eq!(project_agents_md_path(&cwd), PathBuf::from("/tmp/demo/AGENTS.md"));
        assert_eq!(project_skills_dir(&cwd), PathBuf::from("/tmp/demo/.agents/skills"));
    }

    #[test]
    fn test_old_project_paths_use_aemeath_and_claude() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(old_project_config_path(&cwd), PathBuf::from("/tmp/demo/.aemeath/config.json"));
        assert_eq!(old_project_claude_md_path(&cwd), PathBuf::from("/tmp/demo/CLAUDE.md"));
        assert_eq!(old_project_skills_dir(&cwd), PathBuf::from("/tmp/demo/.aemeath/skills"));
    }

    #[test]
    fn test_migrate_dir_once_copies_nested_files_without_overwrite() {
        let base = std::env::temp_dir().join(format!(
            "aemeath_migrate_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = base.join("old");
        let new = base.join("new");
        std::fs::create_dir_all(old.join("nested")).unwrap();
        let mut file = std::fs::File::create(old.join("nested").join("SKILL.md")).unwrap();
        write!(file, "skill").unwrap();

        assert!(migrate_dir_once(&old, &new).unwrap());
        assert_eq!(
            std::fs::read_to_string(new.join("nested").join("SKILL.md")).unwrap(),
            "skill"
        );
        assert!(!migrate_dir_once(&old, &new).unwrap());

        std::fs::remove_dir_all(base).unwrap();
    }
}
```

- [ ] **Step 2: 导出模块并更新注释**

Modify `packages/core/src/config/mod.rs`:

```rust
//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (`~/.agents/aemeath.json` by default)
//! 3. Project config file (`{cwd}/.agents/aemeath.json`)
//! 4. Environment variables
//! 5. Command line arguments
```

Add module line:

```rust
pub mod paths;
```

Update struct doc priority block:

```rust
/// ## Configuration layers (priority: high → low)
/// 1. Command line arguments
/// 2. Environment variables (`AEMEATH_*`)
/// 3. Project config file (`{cwd}/.agents/aemeath.json`)
/// 4. Global config file (`~/.agents/aemeath.json` by default)
/// 5. Built-in defaults
```

Update `test_config_manager_creation` assertion:

```rust
#[test]
fn test_config_manager_creation() {
    let mgr = ConfigManager::new(None);
    assert!(mgr.global_path().to_string_lossy().contains(".agents"));
    assert!(mgr.global_path().to_string_lossy().ends_with("aemeath.json"));
}
```

- [ ] **Step 3: 运行测试确认新模块通过**

Run:

```bash
cargo test -p aemeath-core config::paths config::tests::test_config_manager_creation
```

Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add packages/core/src/config/paths.rs packages/core/src/config/mod.rs
git commit -m "feat(#40): add codex-style config paths"
```

---

### Task 2: 配置文件迁移与新路径读取

**Files:**
- Modify: `packages/core/src/config/manager/mod.rs`
- Modify: `packages/core/src/config/manager/persistence.rs`

- [ ] **Step 1: 修改 ConfigManager 路径**

In `packages/core/src/config/manager/mod.rs`, add import:

```rust
use crate::config::paths;
```

Replace `ConfigManager::new` body:

```rust
pub fn new(project_dir: Option<&Path>) -> Self {
    let global_path = paths::global_config_path();
    let project_path = project_dir.map(paths::project_config_path);

    Self {
        config: RwLock::new(Config::default()),
        global_path,
        project_path,
    }
}
```

- [ ] **Step 2: 增加迁移方法并在 load 前调用**

In the same impl block, add:

```rust
async fn migrate_legacy_configs(&self, project_dir: Option<&Path>) {
    let old_global = paths::old_global_config_path();
    if let Err(err) = paths::migrate_file_once(&old_global, &self.global_path).await {
        log::warn!("配置迁移失败: {err}");
    }

    if let (Some(project_dir), Some(project_path)) = (project_dir, &self.project_path) {
        let old_project = paths::old_project_config_path(project_dir);
        if let Err(err) = paths::migrate_file_once(&old_project, project_path).await {
            log::warn!("项目配置迁移失败: {err}");
        }
    }
}
```

Because `load(&self)` does not currently know `project_dir`, add a private field to `ConfigManager`:

```rust
/// Project directory used for migration and project config resolution
project_dir: Option<PathBuf>,
```

Initialize it in `new`:

```rust
project_dir: project_dir.map(Path::to_path_buf),
```

Call at top of `load` before reading files:

```rust
self.migrate_legacy_configs(self.project_dir.as_deref()).await;
```

- [ ] **Step 3: 更新配置读取错误日志为中文 warning**

Replace silent parse/read failures in `load` with warnings:

```rust
if self.global_path.exists() {
    match tokio::fs::read_to_string(&self.global_path).await {
        Ok(content) => match serde_json::from_str::<Config>(&content) {
            Ok(global_config) => config = Self::merge_config(config, global_config),
            Err(err) => log::warn!("解析全局配置失败 {}: {err}", self.global_path.display()),
        },
        Err(err) => log::warn!("读取全局配置失败 {}: {err}", self.global_path.display()),
    }
}
```

For project config:

```rust
if let Some(project_path) = &self.project_path {
    if project_path.exists() {
        match tokio::fs::read_to_string(project_path).await {
            Ok(content) => match serde_json::from_str::<Config>(&content) {
                Ok(project_config) => config = Self::merge_config(config, project_config),
                Err(err) => log::warn!("解析项目配置失败 {}: {err}", project_path.display()),
            },
            Err(err) => log::warn!("读取项目配置失败 {}: {err}", project_path.display()),
        }
    }
}
```

- [ ] **Step 4: persistence 错误消息改中文**

In `packages/core/src/config/manager/persistence.rs`, replace error messages:

```rust
.map_err(|e| format!("创建配置目录失败: {e}"))?;
```

```rust
.map_err(|e| format!("序列化配置失败: {e}"))?;
```

```rust
.map_err(|e| format!("写入配置失败: {e}"))?;
```

And:

```rust
.ok_or("未设置项目目录")?;
```

- [ ] **Step 5: 添加 ConfigManager 迁移测试**

Append tests in `packages/core/src/config/manager/mod.rs` tests module:

```rust
#[tokio::test]
async fn test_config_manager_uses_agents_project_path() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_config_project_path_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();

    let mgr = ConfigManager::new(Some(&base));
    assert_eq!(mgr.project_path().unwrap(), base.join(".agents").join("aemeath.json"));

    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn test_load_migrates_project_config_once() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_config_migration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let old_dir = base.join(".aemeath");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::write(
        old_dir.join("config.json"),
        r#"{"model":{"name":"migration-model","max_tokens":123,"context_size":456}}"#,
    )
    .unwrap();

    let mgr = ConfigManager::new(Some(&base));
    let loaded = mgr.load().await.unwrap();

    let new_path = base.join(".agents").join("aemeath.json");
    assert!(new_path.exists());
    assert_eq!(loaded.model.name, "migration-model");

    std::fs::write(&new_path, r#"{"model":{"name":"new-model"}}"#).unwrap();
    let loaded = mgr.load().await.unwrap();
    assert_eq!(loaded.model.name, "new-model");

    std::fs::remove_dir_all(base).unwrap();
}
```

- [ ] **Step 6: 运行配置测试**

Run:

```bash
cargo test -p aemeath-core config::manager
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add packages/core/src/config/manager/mod.rs packages/core/src/config/manager/persistence.rs
git commit -m "feat(#40): migrate config files to agents root"
```

---

### Task 3: AGENTS.md 迁移与读取

**Files:**
- Modify: `apps/cli/src/prompt.rs`

- [ ] **Step 1: 引入路径模块并重命名字段注释**

Update import:

```rust
use aemeath_core::config::{paths, MemoryConfig};
```

Update `SystemPromptParts` field comment:

```rust
/// AGENTS.md content, injected separately as a user-context message.
pub claude_md: String,
```

Keep field name `claude_md` to avoid broad call-site churn.

- [ ] **Step 2: 替换调用函数**

In `build_system_prompt_parts`, replace:

```rust
let claude_md = load_claude_md(cwd, hook_runner).await;
```

with:

```rust
let claude_md = load_agents_md(cwd, hook_runner).await;
```

- [ ] **Step 3: 替换 load_claude_md 实现**

Replace the whole `load_claude_md` function with:

```rust
pub async fn load_agents_md(cwd: &PathBuf, hook_runner: &HookRunner) -> String {
    migrate_agents_md(cwd).await;

    let mut parts: Vec<String> = Vec::new();

    let global_path = paths::global_agents_md_path();
    if global_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&global_path).await {
            let file_path_str = global_path.to_string_lossy().to_string();
            hook_runner
                .on_instructions_loaded(&file_path_str, "agents_md")
                .await;
            parts.push(content);
        }
    }

    let project_path = paths::project_agents_md_path(cwd);
    if project_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&project_path).await {
            let file_path_str = project_path.to_string_lossy().to_string();
            hook_runner
                .on_instructions_loaded(&file_path_str, "agents_md")
                .await;
            parts.push(content);
        }
    }

    let mut agents_md = parts.join("\n\n");

    let warnings = aemeath_core::security::scan_content("AGENTS.md", &agents_md);
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!(
                "[Security] {} in {} line {}: {}",
                w.threat_type,
                w.filename,
                w.line_number,
                w.matched_text
            );
        }
        if let Some(prefix) = aemeath_core::security::format_warnings(&warnings) {
            agents_md = format!("{}\n\n{}", prefix, agents_md);
        }
    }

    agents_md
}

async fn migrate_agents_md(cwd: &PathBuf) {
    let old_global = paths::old_global_claude_md_path();
    let new_global = paths::global_agents_md_path();
    if let Err(err) = paths::migrate_file_once(&old_global, &new_global).await {
        log::warn!("全局指令迁移失败: {err}");
    }

    let old_project = paths::old_project_claude_md_path(cwd);
    let new_project = paths::project_agents_md_path(cwd);
    if let Err(err) = paths::migrate_file_once(&old_project, &new_project).await {
        log::warn!("项目指令迁移失败: {err}");
    }
}
```

- [ ] **Step 4: 更新测试引用**

Search in `apps/cli/src/prompt.rs` tests for `load_claude_md` or `CLAUDE.md`. Replace function references with `load_agents_md` and expected filename with `AGENTS.md` where appropriate.

- [ ] **Step 5: 添加项目 AGENTS.md 读取测试**

Append inside tests module in `apps/cli/src/prompt.rs`:

```rust
#[tokio::test]
async fn test_load_agents_md_reads_project_agents_md() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_agents_md_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("AGENTS.md"), "project agents instructions").unwrap();

    let hook_runner = HookRunner::new(None);
    let content = load_agents_md(&base, &hook_runner).await;

    assert!(content.contains("project agents instructions"));

    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn test_load_agents_md_migrates_project_claude_md_once() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_agents_md_migration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("CLAUDE.md"), "old project instructions").unwrap();

    let hook_runner = HookRunner::new(None);
    let content = load_agents_md(&base, &hook_runner).await;

    assert!(base.join("AGENTS.md").exists());
    assert!(content.contains("old project instructions"));

    std::fs::write(base.join("AGENTS.md"), "new project instructions").unwrap();
    let content = load_agents_md(&base, &hook_runner).await;
    assert!(content.contains("new project instructions"));
    assert!(!content.contains("old project instructions"));

    std::fs::remove_dir_all(base).unwrap();
}
```

- [ ] **Step 6: 运行 CLI prompt 测试**

Run:

```bash
cargo test -p aemeath-cli prompt::
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/prompt.rs
git commit -m "feat(#40): migrate instructions to AGENTS.md"
```

---

### Task 4: skills 迁移与新路径读取

**Files:**
- Modify: `packages/core/src/config/skills.rs`
- Modify: `packages/core/src/skill/loader.rs`

- [ ] **Step 1: 更新 skills 配置注释**

In `packages/core/src/config/skills.rs` replace field comment:

```rust
/// Additional directories to load skills from (in addition to `{cwd}/.agents/skills` and `~/.agents/skills`).
/// Supports `~` expansion for home directory.
```

- [ ] **Step 2: 修改 loader import**

At top of `packages/core/src/skill/loader.rs`, add:

```rust
use crate::config::paths;
```

- [ ] **Step 3: 添加 skills 迁移函数**

Before `load_all_skills`, add:

```rust
fn migrate_legacy_skills(cwd: &Path) {
    let old_project = paths::old_project_skills_dir(cwd);
    let new_project = paths::project_skills_dir(cwd);
    if let Err(err) = paths::migrate_dir_once(&old_project, &new_project) {
        log::warn!("项目 skills 迁移失败: {err}");
    }

    let old_global = paths::old_global_skills_dir();
    let new_global = paths::global_skills_dir();
    if let Err(err) = paths::migrate_dir_once(&old_global, &new_global) {
        log::warn!("全局 skills 迁移失败: {err}");
    }
}
```

- [ ] **Step 4: 替换 load_all_skills 路径逻辑**

Replace function body start through global skills block with:

```rust
pub fn load_all_skills(cwd: &Path, extra_dirs: &[PathBuf]) -> HashMap<String, Skill> {
    migrate_legacy_skills(cwd);

    let mut map = HashMap::new();
    let home = dirs::home_dir();

    let project_dir = paths::project_skills_dir(cwd);
    for skill in load_skills_from_dir(&project_dir) {
        map.insert(skill.name.clone(), skill);
    }

    let agents_global = paths::global_skills_dir();
    for skill in load_skills_from_dir(&agents_global) {
        map.entry(skill.name.clone()).or_insert(skill);
    }

    // Extra skill directories from config (lowest priority)
```

Keep the existing extra dirs expansion loop.

- [ ] **Step 5: 更新/新增 tests**

Update existing comments mentioning `~/.aemeath/skills` to `~/.agents/skills`.

Append tests:

```rust
#[test]
fn test_load_all_skills_prefers_project_agents_skills() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_agents_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project_skills = base.join(".agents").join("skills");
    std::fs::create_dir_all(&project_skills).unwrap();
    let mut file = std::fs::File::create(project_skills.join("demo.md")).unwrap();
    write!(file, "---\nname: demo\ndescription: demo\n---\nproject skill").unwrap();

    let skills = load_all_skills(&base, &[]);

    assert!(skills.contains_key("demo"));
    assert_eq!(skills["demo"].source_path, project_skills.join("demo.md"));

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_load_all_skills_migrates_project_aemeath_skills_once() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_skill_migration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let old_skills = base.join(".aemeath").join("skills");
    std::fs::create_dir_all(&old_skills).unwrap();
    let mut file = std::fs::File::create(old_skills.join("legacy.md")).unwrap();
    write!(file, "---\nname: legacy\ndescription: legacy\n---\nlegacy skill").unwrap();

    let skills = load_all_skills(&base, &[]);
    let new_skills = base.join(".agents").join("skills");

    assert!(new_skills.join("legacy.md").exists());
    assert!(skills.contains_key("legacy"));

    let mut file = std::fs::File::create(new_skills.join("modern.md")).unwrap();
    write!(file, "---\nname: modern\ndescription: modern\n---\nmodern skill").unwrap();
    let skills = load_all_skills(&base, &[]);
    assert!(skills.contains_key("modern"));

    std::fs::remove_dir_all(base).unwrap();
}
```

- [ ] **Step 6: 运行 skill 测试**

Run:

```bash
cargo test -p aemeath-core skill::loader
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add packages/core/src/config/skills.rs packages/core/src/skill/loader.rs
git commit -m "feat(#40): migrate skills to agents directories"
```

---

### Task 5: 文档状态、全量验证和收尾

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: 更新 #40 状态**

In `docs/feature/active.md`, update table row #40 status from `待实施` to `待确认`, and append target text:

```text
；已实现一次性迁移：旧路径存在且新路径不存在时复制到新路径，之后仅读写新路径，旧文件不删除不覆盖。
```

In #40 detail section add a short implementation note under core requirements:

```markdown
**实现结果（2026-05-21）**：采用一次性迁移策略。全局配置迁移到 `~/.agents/aemeath.json`，项目配置迁移到 `{cwd}/.agents/aemeath.json`；指令迁移到 `~/.agents/AGENTS.md` 与 `{cwd}/AGENTS.md`；skills 迁移到 `~/.agents/skills` 与 `{cwd}/.agents/skills`。迁移只复制、不删除旧文件、不覆盖已存在的新文件；迁移后运行时只读取新路径。Worktree 下以启动 `cwd` 为边界迁移和读取，不跨 checkout 共享项目配置。
```

- [ ] **Step 2: 运行格式化**

Run:

```bash
cargo fmt
```

Expected: no errors。

- [ ] **Step 3: 运行核心验证**

Run:

```bash
cargo test -p aemeath-core config::paths config::manager skill::loader
```

Expected: PASS。

Run:

```bash
cargo test -p aemeath-cli prompt::
```

Expected: PASS。

Run:

```bash
cargo check
```

Expected: PASS。

- [ ] **Step 4: 检查 diff 与路径残留**

Run:

```bash
git diff --check
```

Expected: no output。

Run:

```bash
git grep -n "CLAUDE.md\|\.aemeath/skills\|\.aemeath/config.json\|~/.aemeath/config.json" -- ':!docs/superpowers/plans/2026-05-21-codex-style-config-migration.md'
```

Expected: only intentional legacy migration references and historical docs remain; no runtime path comments incorrectly claim old paths are primary。

- [ ] **Step 5: Commit**

```bash
git add docs/feature/active.md
git commit -m "docs(#40): mark config migration implemented"
```

- [ ] **Step 6: 最终验证**

Run:

```bash
git status --short
```

Expected: clean working tree。

Run:

```bash
git log --oneline -5
```

Expected: includes the #40 implementation commits。
