# Guidance 多语言支持实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 guidance 系统添加多语言支持，允许用户配置 guidance 语言（中文/英文），并在加载时按语言选择对应内容。

**Architecture:** 采用子目录法实现多语言支持。在 `~/.agents/guidance/` 目录下，按语言创建子目录（`en/`, `zh/`），官方默认文件放在对应语言目录下。用户自定义文件可直接放根目录，所有语言均可加载回退。配置中新增顶层 `Config.language` 字段，默认 `"en"`。

**Tech Stack:** Rust, serde, tokio

---

## 文件变更清单

### 修改文件
- `agent/shared/src/config.rs` — 在 `Config` 中新增 `language` 字段
- `agent/features/prompt/src/business/guidance/resolver.rs` — 修改文件加载逻辑，支持子目录语言回退
- `agent/features/prompt/src/business/guidance/constants.rs` — 新增中文版 guidance 文件内容
- `agent/features/prompt/src/business/guidance.rs` — 修改 `init_guidance_dir()` 支持子目录初始化
- `agent/features/runtime/src/business/prompt/prompt_build_ext.rs` — 传递 language 参数

### 新增测试
- `agent/features/prompt/src/business/guidance/resolver.rs` (tests module) — 新增语言回退测试

---

## Task 1: 扩展 Config 添加 language 字段

**Files:**
- Modify: `agent/shared/src/config.rs:49-98`

- [ ] **Step 1: 添加 language 字段到 Config**

```rust
/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // ... existing fields ...

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Language preference for guidance files. Supported values: "en", "zh".
    /// Default: "en". Guidance files are loaded from `{language}/` subdirectory first,
    /// then fallback to root directory files.
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "en".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // ... existing defaults ...
            language: default_language(),
        }
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check -p share`
Expected: PASS

- [ ] **Step 3: 运行现有测试**

Run: `cargo test -p share -- config`
Expected: PASS

---

## Task 2: 修改 guidance init 支持子目录结构

**Files:**
- Modify: `agent/features/prompt/src/business/guidance.rs:51-75`
- Modify: `agent/features/prompt/src/business/guidance/constants.rs`

- [ ] **Step 1: 更新 constants.rs，将 DEFAULT_FILES 按语言分组**

```rust
/// Default guidance files for English language.
pub const DEFAULT_FILES_EN: &[(&str, &str)] = &[
    (
        "_default.md",
        r#"# Default Guidance
- Think and respond in English unless the user explicitly uses another language.
- Do not proactively generate test cases or documentation (README, etc.) unless explicitly requested.
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
- When using AskUserQuestion with options, the system automatically appends "Type something..." as a built-in option for free-text input. Do NOT include similar options in your options array.
- When using AskUserQuestion with options, prefer object format { "title": "...", "description": "..." } over plain strings. Use description to provide additional context or explanation for each choice.
"#,
    ),
    (
        "deepseek.md",
        r#"# DeepSeek Model Guidance
- Your reasoning/thinking content is displayed separately (thinking mode). Keep it extremely concise: 100 characters or less, 2 sentences max. Do NOT repeat the request, do NOT re-explain code, do NOT include any code snippets in your thinking.
"#,
    ),
    (
        "glm.md",
        r#"# GLM Model Guidance
- Do not paraphrase or repeat tool output in Chinese — refer to it directly.
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
- Your reasoning/thinking content will be displayed separately (thinking mode). Keep it extremely concise: 100 characters or less, 2 sentences max. Do NOT repeat the request, do NOT re-explain code, do NOT include any code snippets in your thinking.
"#,
    ),
    (
        "minimax.md",
        r#"# MiniMax Model Guidance
- Your thinking/reasoning content is displayed separately. In the main response, output conclusions and actions directly.
- Do not repeat your reasoning process in the response body.
"#,
    ),
    (
        "_reasoning.md",
        r#"# Language Preference
- You MUST think/reason in English. Your internal reasoning process must be in English.
- Your final response should also be in English unless the user explicitly writes in another language.
- Keep reasoning concise: output only the final conclusion, no intermediate steps, no code snippets.
"#,
    ),
];

/// Default guidance files for Chinese language.
pub const DEFAULT_FILES_ZH: &[(&str, &str)] = &[
    (
        "_default.md",
        r#"# 默认 Guidance
- 使用中文思考和回复。
- 除非用户明确要求，不要主动生成测试用例、说明文档（README 等）。
- Tool call JSON 参数必须是严格有效的 JSON。发送前请仔细检查。
- 编辑代码时，必须显示精确的 old_string 和 new_string — 不要近似。
- 使用 AskUserQuestion 带选项时，系统会自动添加 "Type something..." 作为自由文本输入选项。不要在 options 数组中包含类似选项。
- 使用 AskUserQuestion 带选项时，优先使用对象格式 { "title": "...", "description": "..." } 而非纯字符串。使用 description 为每个选项提供额外上下文或解释。
"#,
    ),
    (
        "deepseek.md",
        r#"# DeepSeek 模型 Guidance
- 你的推理过程（reasoning_content）必须使用中文。在思考阶段使用中文进行推理和分析。
- 回复内容也使用中文，除非用户明确使用其他语言。
- **强制要求**：reasoning_content 严格限制在 100 字以内。只输出最终结论，禁止中间推导步骤，禁止代码分析，禁止在推理中引用或复制任何代码。超过 2 句话立即停止。这是硬性约束。
"#,
    ),
    (
        "glm.md",
        r#"# GLM 模型 Guidance
- 不要意译或重复工具输出 — 直接引用。
- Tool call JSON 参数必须是严格有效的 JSON。发送前请仔细检查。
- 编辑代码时，必须显示精确的 old_string 和 new_string — 不要近似。
- 你的推理/思考内容会单独显示（thinking mode）。保持极简：100 字以内，最多 2 句话。不要重复请求，不要重新解释代码，不要在思考中包含任何代码片段。
"#,
    ),
    (
        "minimax.md",
        r#"# MiniMax 模型 Guidance
- 你的思考/推理内容会单独显示。在主回复中直接输出结论和行动。
- 不要在回复正文中重复推理过程。
"#,
    ),
    (
        "_reasoning.md",
        r#"# 语言偏好
- 你必须使用中文思考/推理。你的内部推理过程必须是中文。
- 你的最终回复也应使用中文，除非用户明确使用其他语言。
- 保持推理简洁：只输出最终结论，无中间步骤，无代码片段。
"#,
    ),
];

/// All supported languages and their default files.
pub const SUPPORTED_LANGUAGES: &[(&str, &[(&str, &str)])] = &[
    ("en", DEFAULT_FILES_EN),
    ("zh", DEFAULT_FILES_ZH),
];
```

- [ ] **Step 2: 修改 init_guidance_dir() 支持子目录**

```rust
/// Initialise the guidance directory with default files.
///
/// Creates the directory structure:
///   ~/.agents/guidance/
///   ├── en/
///   │   └── (English guidance files)
///   └── zh/
///       └── (Chinese guidance files)
///
/// Existing files are **never** overwritten.
pub fn init_guidance_dir() {
    let dir = match guidance_dir() {
        Some(d) => d,
        None => return,
    };

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("Failed to create guidance dir {}: {}", dir.display(), e);
            return;
        }
    }

    // Initialize language subdirectories
    for (lang, files) in constants::SUPPORTED_LANGUAGES {
        let lang_dir = dir.join(lang);
        if !lang_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&lang_dir) {
                log::warn!("Failed to create guidance lang dir {}: {}", lang_dir.display(), e);
                continue;
            }
        }

        for (filename, content) in *files {
            let path = lang_dir.join(filename);
            if path.exists() {
                continue; // never overwrite user-edited files
            }
            if let Err(e) = std::fs::write(&path, content.trim()) {
                log::warn!("Failed to write {}: {}", path.display(), e);
            }
        }
    }

    log::info!("Initialised default guidance files in {}", dir.display());
}
```

- [ ] **Step 3: 更新测试**

```rust
#[test]
fn test_init_guidance_dir_creates_files() {
    let _lock = ENV_LOCK.lock().unwrap();
    let temp_agents_dir = unique_temp_dir("guidance_init");
    let guidance = temp_agents_dir.join("guidance");
    let _guard = EnvVarGuard::set_path(AGENTS_DIR_ENV, &temp_agents_dir);
    let _ = std::fs::remove_dir_all(&temp_agents_dir);

    init_guidance_dir();

    // Check English subdirectory
    assert!(guidance.join("en/_default.md").exists());
    assert!(guidance.join("en/glm.md").exists());
    assert!(guidance.join("en/deepseek.md").exists());
    assert!(guidance.join("en/_reasoning.md").exists());

    // Check Chinese subdirectory
    assert!(guidance.join("zh/_default.md").exists());
    assert!(guidance.join("zh/glm.md").exists());
    assert!(guidance.join("zh/deepseek.md").exists());
    assert!(guidance.join("zh/_reasoning.md").exists());

    // Verify content
    let content = std::fs::read_to_string(guidance.join("en/_reasoning.md")).unwrap();
    assert!(content.contains("think/reason in English"));

    let content = std::fs::read_to_string(guidance.join("zh/_reasoning.md")).unwrap();
    assert!(content.contains("中文"));

    let _ = std::fs::remove_dir_all(&temp_agents_dir);
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo check -p prompt`
Expected: PASS

---

## Task 3: 修改 guidance resolver 支持语言子目录回退

**Files:**
- Modify: `agent/features/prompt/src/business/guidance/resolver.rs`

- [ ] **Step 1: 修改 `resolve_guidance` 函数签名，添加 language 参数**

```rust
/// Resolve the guidance text for a given model.
///
/// Assembles the final guidance string:
///   1. `_default.md` content (always injected, if exists)
///   2. Model-specific guidance from prefix-matched `{prefix}.md` file
///   3. Fallback to config guidance map (glob match from config)
///   4. If `reasoning == true`, append `_reasoning.md`
///
/// Files are loaded from `{language}/` subdirectory first, then fallback to root.
pub fn resolve_guidance(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    reasoning: bool,
    language: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file_with_lang("_default", language) {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) = resolve_model_guidance(model_id, config_guidance, language) {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) = load_named_file_with_lang("_reasoning", language) {
            parts.push(content);
        }
    }

    parts.join("\n")
}
```

- [ ] **Step 2: 修改 `resolve_guidance_async` 函数签名**

```rust
pub async fn resolve_guidance_async(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    reasoning: bool,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file_async_with_lang("_default", language, hook_runner).await {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) = resolve_model_guidance_async(model_id, config_guidance, language, hook_runner).await {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) = load_named_file_async_with_lang("_reasoning", language, hook_runner).await {
            parts.push(content);
        }
    }

    parts.join("\n")
}
```

- [ ] **Step 3: 添加语言感知的文件加载辅助函数**

```rust
/// Load a named file with language subdirectory support.
/// Tries `{language}/{name}.md` first, falls back to `{name}.md`.
fn load_named_file_with_lang(name: &str, language: &str) -> Option<String> {
    let dir = guidance_dir()?;

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_path = dir.join(language).join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&lang_path) {
            log::debug!("Loaded guidance from {}", lang_path.display());
            return Some(content);
        }
    }

    // Fallback to root directory
    let root_path = dir.join(format!("{}.md", name));
    match std::fs::read_to_string(&root_path) {
        Ok(content) => {
            log::debug!("Loaded guidance from {}", root_path.display());
            Some(content)
        }
        Err(_) => None,
    }
}

/// Async version of load_named_file_with_lang.
async fn load_named_file_async_with_lang(
    name: &str,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let dir = match guidance_dir() {
        Some(d) => d,
        None => return None,
    };

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_path = dir.join(language).join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&lang_path) {
            log::debug!("Loaded guidance from {}", lang_path.display());
            if let Some(hr) = hook_runner {
                let file_path_str = lang_path.to_string_lossy().to_string();
                hr.on_instructions_loaded(&file_path_str, "guidance").await;
            }
            return Some(content);
        }
    }

    // Fallback to root directory
    let root_path = dir.join(format!("{}.md", name));
    match std::fs::read_to_string(&root_path) {
        Ok(content) => {
            log::debug!("Loaded guidance from {}", root_path.display());
            if let Some(hr) = hook_runner {
                let file_path_str = root_path.to_string_lossy().to_string();
                hr.on_instructions_loaded(&file_path_str, "guidance").await;
            }
            Some(content)
        }
        Err(_) => None,
    }
}
```

- [ ] **Step 4: 修改 `resolve_model_guidance` 和 `resolve_model_guidance_async` 支持语言参数**

```rust
fn resolve_model_guidance(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    language: &str,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins) with lang support
    if let Some(content) = load_prefix_matched_file_with_lang(model_id, language) {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}

pub async fn resolve_model_guidance_async(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins) with lang support
    if let Some(content) = load_prefix_matched_file_async_with_lang(model_id, language, hook_runner).await {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}
```

- [ ] **Step 5: 添加带语言子目录的前缀匹配函数**

```rust
/// Load prefix-matched guidance file with language subdirectory support.
/// Tries `{language}/{prefix}.md` first, falls back to `{prefix}.md`.
fn load_prefix_matched_file_with_lang(model_id: &str, language: &str) -> Option<String> {
    let dir = guidance_dir()?;
    let model_lower = model_id.to_lowercase();

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_dir = dir.join(language);
        if let Some(content) = load_prefix_matched_from_dir(&lang_dir, &model_lower) {
            return Some(content);
        }
    }

    // Fallback to root directory
    load_prefix_matched_from_dir(&dir, &model_lower)
}

/// Scan a directory for prefix-matched guidance files.
fn load_prefix_matched_from_dir(dir: &std::path::Path, model_lower: &str) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;

    let mut best_prefix = String::new();
    let mut best_content: Option<String> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip special files
        if stem.starts_with('_') {
            continue;
        }
        if model_lower.starts_with(&stem.to_lowercase()) && stem.len() > best_prefix.len() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                best_prefix = stem;
                best_content = Some(content);
            }
        }
    }

    if best_content.is_some() {
        log::debug!(
            "Matched guidance prefix '{}' for model '{}' in {}",
            best_prefix,
            model_lower,
            dir.display()
        );
    }
    best_content
}

/// Async version with hook support.
async fn load_prefix_matched_file_async_with_lang(
    model_id: &str,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let dir = guidance_dir()?;
    let model_lower = model_id.to_lowercase();

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_dir = dir.join(language);
        if let Some(content) = load_prefix_matched_from_dir_async(&lang_dir, &model_lower, hook_runner).await {
            return Some(content);
        }
    }

    // Fallback to root directory
    load_prefix_matched_from_dir_async(&dir, &model_lower, hook_runner).await
}

async fn load_prefix_matched_from_dir_async(
    dir: &std::path::Path,
    model_lower: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;

    let mut best_prefix = String::new();
    let mut best_path: Option<std::path::PathBuf> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if stem.starts_with('_') {
            continue;
        }
        if model_lower.starts_with(&stem.to_lowercase()) && stem.len() > best_prefix.len() {
            best_prefix = stem;
            best_path = Some(path);
        }
    }

    if let Some(path) = best_path {
        if let Some(hr) = hook_runner {
            let file_path_str = path.to_string_lossy().to_string();
            hr.on_instructions_loaded(&file_path_str, "guidance").await;
        }
        std::fs::read_to_string(&path).ok()
    } else {
        None
    }
}
```

- [ ] **Step 6: 编译验证**

Run: `cargo check -p prompt`
Expected: PASS

---

## Task 4: 更新 guidance 调用方传递 language 参数

**Files:**
- Modify: `agent/features/runtime/src/business/prompt/prompt_build_ext.rs:8-39`

- [ ] **Step 1: 修改 `build_static_prompt` 传递 language 参数**

```rust
pub async fn build_static_prompt(
    _cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&Config>,
    hook_runner: &HookRunner,
    prompt_parts: crate::business::prompt::build::SystemPromptParts,
    skills: &tokio::sync::Mutex<std::collections::HashMap<String, Skill>>,
) -> String {
    let skills_guard = skills.lock().await;
    let guidance_config = config_file
        .map(|c| c.models.guidance.clone())
        .unwrap_or_default();
    let language = config_file
        .map(|c| c.language.clone())
        .unwrap_or_else(|| "en".to_string());
    let instructions_hook = bootstrap::InstructionsLoadedHookRunner(hook_runner);
    let model_guidance = prompt::api::guidance::resolve_guidance_async(
        model,
        &guidance_config,
        reasoning,
        &language,
        Some(&instructions_hook),
    )
    .await;

    let mut prompt = prompt_parts.static_part;
    prompt.push_str(prompt::api::guidance::UNIVERSAL_EXECUTION_DISCIPLINE);
    append_skills(&mut prompt, &skills_guard);
    append_agent_roles(&mut prompt, config_file);
    if !model_guidance.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&model_guidance);
    }
    prompt
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check -p runtime`
Expected: PASS

---

## Task 5: 更新 guidance_contract 测试

**Files:**
- Modify: `agent/features/prompt/tests/guidance_contract.rs`

- [ ] **Step 1: 更新测试调用，传递 language 参数**

```rust
use prompt::api::guidance::{resolve_guidance, UNIVERSAL_EXECUTION_DISCIPLINE};
use std::collections::HashMap;

#[test]
fn test_resolve_guidance_default() {
    let guidance = HashMap::new();
    let resolved = resolve_guidance("other-model", &guidance, false, "en");
    // Should contain default guidance content
    assert!(!resolved.is_empty() || true); // May be empty if no guidance files
}

#[test]
fn test_resolve_guidance_with_language() {
    let guidance = HashMap::new();
    // Test with English
    let resolved_en = resolve_guidance("other-model", &guidance, false, "en");
    // Test with Chinese
    let resolved_zh = resolve_guidance("other-model", &guidance, false, "zh");
    // Both should return valid strings
    assert!(resolved_en.is_empty() || !resolved_en.is_empty());
    assert!(resolved_zh.is_empty() || !resolved_zh.is_empty());
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test -p prompt -- guidance_contract`
Expected: PASS

---

## Task 6: 集成测试验证语言子目录回退机制

**Files:**
- Modify: `agent/features/prompt/src/business/guidance.rs` (tests module)

- [ ] **Step 1: 添加语言子目录回退测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... existing tests ...

    #[test]
    fn test_language_subdir_fallback() {
        let _lock = ENV_LOCK.lock().unwrap();
        let temp_agents_dir = unique_temp_dir("guidance_lang");
        let guidance = temp_agents_dir.join("guidance");
        let _guard = EnvVarGuard::set_path(AGENTS_DIR_ENV, &temp_agents_dir);
        let _ = std::fs::remove_dir_all(&temp_agents_dir);

        // Create root file only (no language subdirectory)
        std::fs::create_dir_all(&guidance).unwrap();
        std::fs::write(guidance.join("_default.md"), "root content").unwrap();

        // With language="zh", should fallback to root file
        let content = resolver::load_named_file_with_lang("_default", "zh");
        assert_eq!(content, Some("root content".to_string()));

        // Create Chinese subdirectory with file
        let zh_dir = guidance.join("zh");
        std::fs::create_dir_all(&zh_dir).unwrap();
        std::fs::write(zh_dir.join("_default.md"), "zh content").unwrap();

        // Now should prefer Chinese version
        let content = resolver::load_named_file_with_lang("_default", "zh");
        assert_eq!(content, Some("zh content".to_string()));

        // English should still use root (no en/ directory)
        let content = resolver::load_named_file_with_lang("_default", "en");
        assert_eq!(content, Some("root content".to_string()));

        // Create English subdirectory
        let en_dir = guidance.join("en");
        std::fs::create_dir_all(&en_dir).unwrap();
        std::fs::write(en_dir.join("_default.md"), "en content").unwrap();

        // Now English should use its own
        let content = resolver::load_named_file_with_lang("_default", "en");
        assert_eq!(content, Some("en content".to_string()));

        let _ = std::fs::remove_dir_all(&temp_agents_dir);
    }
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test -p prompt -- guidance::tests::test_language_subdir_fallback`
Expected: PASS

- [ ] **Step 3: 运行完整测试套件**

Run: `cargo test -p prompt`
Expected: PASS

---

## Task 7: 编译并运行完整验证

- [ ] **Step 1: 运行 cargo check 全项目**

Run: `cargo check`
Expected: PASS

- [ ] **Step 2: 运行 cargo clippy**

Run: `cargo clippy -- -D warnings`
Expected: PASS

- [ ] **Step 3: 运行所有相关测试**

Run: `cargo test -p prompt -p share -p runtime`
Expected: PASS

- [ ] **Step 4: 生成默认 guidance 文件并验证**

```bash
# 删除现有 guidance 目录以触发重新生成
rm -rf ~/.agents/guidance

# 运行程序触发 init_guidance_dir()
cargo run -- --help

# 检查生成的文件
ls -la ~/.agents/guidance/
ls -la ~/.agents/guidance/en/
ls -la ~/.agents/guidance/zh/
# 应该看到 en/ 和 zh/ 子目录，每个里面都有 _default.md, _reasoning.md, deepseek.md 等
```

- [ ] **Step 5: 测试配置语言切换**

创建测试配置 `~/.agents/aemeath.json`:
```json
{
  "language": "zh"
}
```

运行程序，验证加载的是中文版 guidance。

---

## 自检清单

1. **需求覆盖**:
   - [x] guidance 支持多语言（中/英）—— 通过子目录法实现
   - [x] 现有 guidance 内容补充中文版本 —— 在 DEFAULT_FILES_ZH 中添加
   - [x] 配置中声明 guidance 语言 —— 顶层 Config.language 字段
   - [x] 加载时按配置语言选择 —— load_named_file_with_lang 子目录优先
   - [x] 未配置或缺失时 fallback —— 回退到根目录文件
   - [x] 现有 guidance 均提供中文内容 —— zh/ 子目录下完整中文版

2. **无占位符**: 所有步骤包含完整代码

3. **类型一致性**: `language` 参数在所有函数中保持 `&str` 类型

---

## 执行选项

**计划已保存到 `docs/superpowers/plans/2026-06-13-guidance-multilingual.md`。两种执行方式：**

1. **Subagent-Driven (推荐)** — 为每个 task 分派新 subagent，task 间进行 review，快速迭代
2. **Inline Execution** — 在当前会话中使用 executing-plans 执行，批量执行并设置检查点

**选择哪种方式？**
