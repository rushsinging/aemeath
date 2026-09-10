//! Filesystem Skill adapter（Issue #1438）。
//!
//! 同时实现 [`SkillCatalogPort`] 与 [`SkillLoadPort`]：Catalog 只返回元数据，
//! Load 在调用时按 identity 重新发现并读取单个 Skill 正文。
//!
//! adapter 不捕获 project root，也不缓存 Skill 正文。每次调用都从 query 的
//! project root、extra dirs 与 available tools 重建可见集合；同名 Skill 按
//! project `.claude` → project `.agents` → global → extra → builtin 优先级去重。
//! 目录不存在正常为空；扫描中遇到的无关入口错误只记录 warn 并跳过，绝不阻断
//! 其他 Skill 的加载；仅当被请求 identity 的入口自身读取或解析失败时，`load`
//! 才返回对应的 typed [`SkillError`]。

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::skill_pl::{
    LoadedSkill, SkillCatalogPort, SkillDescriptor, SkillError, SkillLoadPort, SkillLoadQuery,
    SkillQuery, SkillSource, SkillSourceKind,
};

// ── adapter ────────────────────────────────────────────────────────────

/// 文件系统 Skill adapter。
///
/// 无状态、无全局缓存；list/load 都从 query 快照重新发现文件。
/// 构造器只持有全局 `~/.agents/skills` 根目录。
pub struct FilesystemSkillAdapter {
    /// 全局 `~/.agents/skills`（构造器注入，生产默认见 [`Self::default`]）。
    global: PathBuf,
}

impl FilesystemSkillAdapter {
    /// 显式注入全局根目录构造 adapter。
    ///
    /// `global` 通常是 `~/.agents/skills`；测试可用任意临时目录。adapter
    /// **不**捕获任何 project root——它们由每次 query 提供。
    pub fn new(global: PathBuf) -> Self {
        Self { global }
    }

    /// 加载一个显式路径的 Skill，用于 typed I/O 契约测试。
    pub async fn load_one(path: &Path, kind: SkillSourceKind) -> Result<LoadedSkill, SkillError> {
        let raw = parse_skill_file(path, kind)?;
        Ok(raw.into_loaded())
    }

    /// 按优先级顺序发现所有 Skill（含内置 commit），返回带 typed 错误的
    /// 原始列表（未去重、未排序、未过滤）。
    ///
    /// 不存在的目录视为空（非错误）；只有**已扫描到**的文件读取 / 解析失败
    /// 才产生 typed [`SkillError`]。
    fn discover_all(&self, query: &SkillQuery) -> Vec<Result<RawSkill, SkillError>> {
        use share::config::paths;

        let project_claude = paths::project_claude_skills_dir(&query.project_root);
        let project_agents = paths::project_skills_dir(&query.project_root);
        let extra: Vec<PathBuf> = query
            .extra_dirs
            .iter()
            .map(|d| paths::expand_home(d))
            .collect();

        let mut out: Vec<Result<RawSkill, SkillError>> = Vec::new();

        // 1. project .claude/skills（最高）
        scan_dir(&project_claude, SkillSourceKind::ProjectClaude, &mut out);
        // 2. project .agents/skills
        scan_dir(&project_agents, SkillSourceKind::ProjectAgents, &mut out);
        // 3. global ~/.agents/skills（构造器注入）
        scan_dir(&self.global, SkillSourceKind::Global, &mut out);
        // 4. extra dirs（按顺序，最低）
        for dir in &extra {
            scan_dir(dir, SkillSourceKind::Extra, &mut out);
        }
        // 5. builtin commit（最低）
        out.push(Ok(builtin_commit_skill()));

        out
    }

    /// 发现、去重（先到先得，保持优先级）、过滤（requires_tools /
    /// fallback_for）、并按 stable key 排序。
    ///
    /// 返回可用的 [`RawSkill`] 列表与扫描中遇到的入口错误；错误如何处置
    /// （warn 跳过还是归因返回）由调用方决定，扫描本身绝不因单个坏入口中止。
    fn collect_raws(&self, query: &SkillQuery) -> (Vec<RawSkill>, Vec<SkillError>) {
        let mut seen: HashSet<String> = HashSet::new();
        let mut acc: Vec<RawSkill> = Vec::new();
        let mut errors: Vec<SkillError> = Vec::new();
        for res in self.discover_all(query) {
            match res {
                Ok(raw) => {
                    if seen.insert(raw.name.clone()) {
                        acc.push(raw);
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        // 过滤需要完整 Skill 名集合（fallback_for 据此判断）。
        let all_names: BTreeSet<String> = acc.iter().map(|r| r.name.clone()).collect();
        let mut filtered: Vec<RawSkill> = acc
            .into_iter()
            .filter(|raw| raw.is_visible(&query.available_tools, &all_names))
            .collect();

        // 稳定排序：按 name（stable_key）。
        filtered.sort_by(|a, b| a.name.cmp(&b.name));
        (filtered, errors)
    }

    /// 记录扫描阶段跳过的坏入口（catalog 与 load 共用，保证无关错误留痕）。
    fn log_skipped_entries(errors: &[SkillError]) {
        for err in errors {
            log::warn!(target: crate::LOG_TARGET, "skill discovery skipped a file: {err}");
        }
    }
}

impl Default for FilesystemSkillAdapter {
    /// 生产默认：全局根目录沿用共享内核 `share::config::paths`（解析
    /// `AEMEATH_AGENTS_DIR` 与 `$HOME`）。
    fn default() -> Self {
        Self::new(share::config::paths::global_skills_dir())
    }
}

impl SkillCatalogPort for FilesystemSkillAdapter {
    fn list(&self, query: SkillQuery) -> Vec<SkillDescriptor> {
        // catalog 采用 best-effort：坏入口只留痕，不阻断其余 Skill 的元数据。
        let (raws, errors) = self.collect_raws(&query);
        Self::log_skipped_entries(&errors);
        raws.into_iter().map(RawSkill::into_descriptor).collect()
    }
}

#[async_trait]
impl SkillLoadPort for FilesystemSkillAdapter {
    async fn load(&self, query: SkillLoadQuery) -> Result<LoadedSkill, SkillError> {
        let identity = query.identity().to_string();
        let (raws, errors) = self.collect_raws(&query.catalog_query());
        Self::log_skipped_entries(&errors);

        if let Some(raw) = raws
            .into_iter()
            .find(|raw| identity_matches(raw, &identity))
        {
            return Ok(raw.into_loaded());
        }

        // 目标不在可用集合中：若某个失败入口的派生名与 identity 匹配，说明
        // 正是被请求的 Skill 自身损坏，返回其 typed 错误而非笼统的 NotFound；
        // 其余错误属于无关入口，不得影响本次加载结果。
        if let Some(err) = errors
            .iter()
            .find(|err| error_entry_matches_identity(err, &identity))
        {
            return Err(err.clone());
        }
        Err(SkillError::not_found(identity))
    }
}

/// identity 命中规则：canonical name 或任一 alias（大小写不敏感）。
fn identity_matches(raw: &RawSkill, identity: &str) -> bool {
    raw.name.eq_ignore_ascii_case(identity)
        || raw
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(identity))
}

/// 从失败入口路径推断其派生 identity，判断坏的是否正是被请求的 Skill：
/// 目录入口取目录名，`SKILL.md` 取父目录名，扁平 `.md` 入口取文件 stem。
/// 带 package namespace 的 identity（如 `superpowers:brainstorming`）取末段比较。
/// 注意：损坏的文件无法解析出 frontmatter alias，此处只比较入口路径派生名。
fn error_entry_matches_identity(err: &SkillError, identity: &str) -> bool {
    let path = match err {
        SkillError::ReadFailed { path, .. } | SkillError::ParseFailed { path, .. } => path,
        SkillError::InvalidIdentity { .. } | SkillError::NotFound { .. } => return false,
    };
    let entry_path = Path::new(path);
    let entry_name = match entry_path.file_name().and_then(|name| name.to_str()) {
        Some(name) if name.eq_ignore_ascii_case("SKILL.md") => entry_path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
        Some(_)
            if entry_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md")) =>
        {
            entry_path.file_stem().and_then(|stem| stem.to_str())
        }
        Some(name) => Some(name),
        None => None,
    };
    let Some(entry_name) = entry_name else {
        return false;
    };
    let identity_tail = identity.rsplit(':').next().unwrap_or(identity);
    entry_name.eq_ignore_ascii_case(identity) || entry_name.eq_ignore_ascii_case(identity_tail)
}

// ── 内部：原始解析结果 ─────────────────────────────────────────────────

/// 单个 Skill 文件解析后的完整原始数据：descriptor 字段 + 物化正文 +
/// frontmatter 过滤声明。
#[derive(Debug, Clone)]
struct RawSkill {
    name: String,
    description: String,
    aliases: Vec<String>,
    /// 显式 Slash 投影；package namespace Skill 使用完整 qualified identity。
    slash_command: Option<String>,
    /// 仅当 Slash 投影存在时随之公开的合法别名。
    slash_aliases: Vec<String>,
    /// 面向用户/模型的可选参数提示，不定义业务 schema。
    argument_hint: Option<String>,
    source: SkillSource,
    content: String,
    /// frontmatter `requires_tools`：非空时要求所列工具全部出现在
    /// `available_tools`，否则隐藏该 Skill。
    requires_tools: Vec<String>,
    /// frontmatter `fallback_for`：若所列 Skill 名任一出现在完整名集合中，
    /// 隐藏本 fallback Skill。
    fallback_for: Vec<String>,
}

impl RawSkill {
    fn into_descriptor(self) -> SkillDescriptor {
        SkillDescriptor::new(
            self.name,
            self.description,
            self.source,
            self.aliases,
            self.slash_command,
            self.slash_aliases,
            self.argument_hint,
        )
    }

    fn into_loaded(self) -> LoadedSkill {
        LoadedSkill::from_content(self.name, self.content, self.source)
    }

    /// 依据 `available_tools` 与完整 Skill 名集合判断本 Skill 是否可见。
    fn is_visible(&self, available_tools: &BTreeSet<String>, all_names: &BTreeSet<String>) -> bool {
        // requires_tools：非空且任一所列工具缺失 → 隐藏。
        if !self.requires_tools.is_empty()
            && !self
                .requires_tools
                .iter()
                .all(|t| available_tools.contains(t))
        {
            return false;
        }
        // fallback_for：所列主 Skill 任一存在 → 隐藏 fallback。
        if self.fallback_for.iter().any(|s| all_names.contains(s)) {
            return false;
        }
        true
    }
}

/// 恢复 `requires_tools` / `fallback_for` 与 Slash 投影：前两者分别声明
/// 工具依赖和 fallback 关系；`slash_command` / `slash_aliases` 则显式控制
/// 是否及如何投影为用户 Slash Command。adapter 不从 Skill identity 推导 Slash。
#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    /// 可选 Slash Command 名；省略时普通 Skill 默认使用自身 identity，package
    /// namespace 会在 `apply_namespace` 阶段发布完整 qualified identity。
    #[serde(default)]
    slash_command: Option<String>,
    /// `slash_command` 的用户可输入别名，不与 identity aliases 混用。
    #[serde(default)]
    slash_aliases: Vec<String>,
    /// 可选参数提示；只进入 metadata，不参与 Skill Tool input schema。
    #[serde(default, rename = "argument-hint", alias = "argument_hint")]
    argument_hint: Option<String>,
    /// 所需工具名；任一缺失则该 Skill 不可见。
    #[serde(default)]
    requires_tools: Vec<String>,
    /// 本 Skill 是哪些（完整名）Skill 的 fallback；主 Skill 存在则隐藏。
    #[serde(default)]
    fallback_for: Vec<String>,
}

// ── 内部：发现（目录遍历） ─────────────────────────────────────────────

/// 扫描一个 Skill 根目录：
/// - 根目录直接 `.md` 保留为历史扁平兼容入口；
/// - 普通子目录只识别 `<name>/SKILL.md`；
/// - package 子目录只识别 `<package>/skills/<name>/SKILL.md`。
/// - 子目录同时含 `SKILL.md` 与 `skills/` 时，直接入口优先。
///
/// 子目录中的其他 Markdown 是 Skill 资源，绝不作为独立入口解析。
/// **目录不存在视为空（非错误）**；真实入口的读取 / 解析失败产生 typed
/// [`SkillError`]。
fn scan_dir(dir: &Path, kind: SkillSourceKind, out: &mut Vec<Result<RawSkill, SkillError>>) {
    let Some(entries) = read_skill_directory(dir, out) else {
        return;
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_read_error(dir, error, out);
                continue;
            }
        };
        let path = entry.path();
        let Some(metadata) = discovered_metadata(&path, out) else {
            continue;
        };
        if metadata.is_file() && path.extension().is_some_and(|extension| extension == "md") {
            // 历史兼容：仅 Skill 根目录的直接 Markdown 文件仍是入口。
            out.push(parse_skill_file(&path, kind).map(|raw| apply_namespace(raw, None)));
        } else if metadata.is_dir() {
            if scan_skill_entry(&path, kind, None, out) {
                continue;
            }
            let skills_child = path.join("skills");
            match std::fs::metadata(&skills_child) {
                Ok(metadata) if metadata.is_dir() => {
                    let namespace = path.file_name().map(|name| name.to_string_lossy());
                    scan_skill_directories(&skills_child, kind, namespace.as_deref(), out);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => push_read_error(&skills_child, error, out),
            }
        }
    }
}

fn scan_skill_directories(
    dir: &Path,
    kind: SkillSourceKind,
    namespace: Option<&str>,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) {
    let Some(entries) = read_skill_directory(dir, out) else {
        return;
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_read_error(dir, error, out);
                continue;
            }
        };
        let path = entry.path();
        let Some(metadata) = discovered_metadata(&path, out) else {
            continue;
        };
        if metadata.is_dir() {
            scan_skill_entry(&path, kind, namespace, out);
        }
    }
}

fn scan_skill_entry(
    skill_dir: &Path,
    kind: SkillSourceKind,
    namespace: Option<&str>,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) -> bool {
    let entry = skill_dir.join("SKILL.md");
    match std::fs::symlink_metadata(&entry) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            out.push(parse_skill_file(&entry, kind).map(|raw| apply_namespace(raw, namespace)));
            true
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            push_read_error(&entry, error, out);
            true
        }
    }
}

fn read_skill_directory(
    dir: &Path,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) -> Option<std::fs::ReadDir> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            push_read_error(dir, error, out);
            None
        }
    }
}

fn discovered_metadata(
    path: &Path,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) -> Option<std::fs::Metadata> {
    match std::fs::metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            push_read_error(path, error, out);
            None
        }
    }
}

fn push_read_error(
    path: &Path,
    error: std::io::Error,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) {
    out.push(Err(SkillError::read_failed(
        path.to_string_lossy(),
        error.to_string(),
    )));
}

/// 应用命名空间前缀（skill package）。原 name 进入 aliases。
fn apply_namespace(mut raw: RawSkill, namespace: Option<&str>) -> RawSkill {
    if let Some(ns) = namespace {
        if !ns.is_empty() {
            raw.aliases.push(raw.name.clone());
            raw.name = format!("{ns}:{}", raw.name);
            raw.slash_command = Some(raw.name.clone());
            raw.slash_aliases.clear();
        }
    }
    raw
}

// ── 内部：单文件解析（typed） ──────────────────────────────────────────

const BUILTIN_COMMIT_URI: &str = "aemeath-builtin://commit";

/// 解析单个 Skill 文件（frontmatter + 正文），失败返回 typed 错误。
fn parse_skill_file(path: &Path, kind: SkillSourceKind) -> Result<RawSkill, SkillError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| SkillError::read_failed(path.to_string_lossy(), e.to_string()))?;

    let frontmatter_str = extract_frontmatter(&text)
        .map_err(|reason| SkillError::parse_failed(path.to_string_lossy(), reason))?;

    let fm: Frontmatter = serde_yml::from_str(frontmatter_str)
        .map_err(|e| SkillError::parse_failed(path.to_string_lossy(), e.to_string()))?;

    let body = extract_body(&text);

    // 名称解析优先级：frontmatter name > 通用文件名用父目录名 > 文件 stem。
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    let file_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_generic = file_stem.eq_ignore_ascii_case("skill")
        || file_stem.eq_ignore_ascii_case("index")
        || file_stem.eq_ignore_ascii_case("readme");

    let name = if !fm.name.is_empty() {
        fm.name
    } else if is_generic {
        dir_name.clone().unwrap_or(file_stem.clone())
    } else {
        file_stem.clone()
    };

    // 自动把父目录名加为 alias（若与 name 不同且尚未存在）。
    let mut aliases = fm.aliases;
    if let Some(ref dir) = dir_name {
        if dir.as_str() != name && !aliases.contains(dir) {
            aliases.push(dir.clone());
        }
    }

    Ok(RawSkill {
        slash_command: fm.slash_command.or_else(|| Some(name.clone())),
        slash_aliases: fm.slash_aliases,
        argument_hint: fm.argument_hint,
        name,
        description: fm.description,
        aliases,
        source: SkillSource::file(kind, path.to_string_lossy().to_string()),
        content: body,
        requires_tools: fm.requires_tools,
        fallback_for: fm.fallback_for,
    })
}

/// 抽取 frontmatter YAML 文本（首尾 `---` 之间）。失败返回中文原因。
fn extract_frontmatter(text: &str) -> Result<&str, &'static str> {
    if !text.starts_with("---") {
        return Err("缺少 YAML frontmatter 起始标记");
    }
    let rest = &text[3..]; // allow unsafe_text_op: fixed ascii prefix "---"
    let end = rest
        .find("---")
        .ok_or("YAML frontmatter 未闭合（缺少结束 `---`）")?;
    Ok(rest[..end].trim()) // allow unsafe_text_op: find offset (char boundary)
}

/// 抽取 frontmatter 之后的 markdown 正文。
fn extract_body(text: &str) -> String {
    if !text.starts_with("---") {
        return text.to_string();
    }
    let rest = &text[3..]; // allow unsafe_text_op: fixed ascii prefix "---"
    match rest.find("---") {
        Some(end) => rest[end + 3..].trim().to_string(),
        None => String::new(),
    }
}

// ── 内置 commit Skill ──────────────────────────────────────────────────

fn builtin_commit_skill() -> RawSkill {
    RawSkill {
        name: "commit".to_string(),
        description: "Create a git commit using the repository's Commit Style Context".to_string(),
        aliases: vec!["git-commit".to_string()],
        slash_command: Some("commit".to_string()),
        slash_aliases: vec!["git-commit".to_string()],
        argument_hint: None,
        source: SkillSource::builtin(BUILTIN_COMMIT_URI),
        content: builtin_commit_body().to_string(),
        requires_tools: Vec::new(),
        fallback_for: Vec::new(),
    }
}

fn builtin_commit_body() -> &'static str {
    r#"# Built-in commit skill

Use this skill whenever you need to create a git commit.

## Required workflow

1. Inspect the working tree with `git status --short --branch`.
2. Inspect repository commit style before writing a message. Prefer commits with AI co-author trailers:
   `git log --format=%B --grep='Co-Authored-By' -n 20`
3. If there are no useful co-author examples, sample recent ordinary commits with a small limit.
4. Inspect staged and unstaged changes enough to understand the commit scope.
5. Generate a commit message that matches this repository's Commit Style Context.
6. Do not invent human co-authors.
7. When an AI co-author trailer is appropriate, use the exact trailer supplied by the current system prompt.
8. Run `git commit` with the generated message.

## Safety rules

- Do not stage unrelated files unless the user explicitly asks.
- Do not amend unless the user explicitly asks.
- If the working tree contains unrelated user changes, report them and commit only the intended paths.
"#
}
