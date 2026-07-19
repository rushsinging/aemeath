//! Filesystem Skill adapter（Issue #912）。
//!
//! 同时实现 [`SkillCatalogPort`] 与 [`SkillMaterializationPort`]，从文件
//! 系统发现并物化 Skill。
//!
//! # 发现根目录（每次调用从 query 快照推导）
//!
//! adapter **不**捕获 project roots——project `.claude/skills`、
//! `.agents/skills` 与 `extra_dirs` 都由每次 `list` /
//! `materialize_available` 收到的 query 快照（[`SkillQuery`] /
//! [`SkillMaterializationQuery`]）推导。构造器只接受全局根目录
//! （`~/.agents/skills`），并提供生产默认（[`FilesystemSkillAdapter::default`]）。
//!
//! 优先级（高 → 低）：
//!
//! - 项目级 `{project_root}/.claude/skills`（最高优先级）
//! - 项目级 `{project_root}/.agents/skills`
//! - 全局 `~/.agents/skills`（构造器注入）
//! - query 提供的 `extra_dirs`（最低，按顺序扫描）
//! - 内置 `commit` Skill（最低）
//!
//! 设计约束（见 Issue #912 / `specs/tools.md`）：
//!
//! - 同名 Skill 按上述优先级「先到先得」去重；
//! - 输出按 stable key 稳定排序；
//! - revision 是确定性内容 revision（见
//!   [`SkillMaterializationRevision::from_fragments`]）；
//! - **无进程级全局单槽缓存**——每次调用重新读取文件系统，因此新增 /
//!   修改文件在下一次调用立即可见；
//! - frontmatter 可声明 `requires_tools` / `fallback_for`，adapter 依据
//!   query 的 `available_tools` 与完整 Skill 名集合过滤；
//! - `materialize_available` 对**已扫描文件**的读取 / 解析失败返回第一个
//!   typed [`SkillError`]（不静默跳过）；**不存在目录正常为空**（非错误）。
//!
//! Skill 不是 Tool：本 adapter 不经 `ToolExecutionPort`，仅产出值类型
//! `PromptFragment` 供 Context Management 消费。

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::skill_pl::{
    CacheHint, PromptFragment, SkillCatalogPort, SkillDescriptor, SkillError,
    SkillMaterializationPort, SkillMaterializationQuery, SkillMaterializationRevision,
    SkillMaterializationSnapshot, SkillQuery, SkillSource, SkillSourceKind,
};

// ── adapter ────────────────────────────────────────────────────────────

/// 文件系统 Skill adapter。
///
/// 无状态、无全局缓存：每次 [`SkillCatalogPort::list`] /
/// [`SkillMaterializationPort::materialize_available`] 都从对应 query 快照
/// 推导 project `.claude/skills`、`.agents/skills` 与 `extra_dirs`，并重新
/// 读取文件系统。构造器只持有全局根目录（`~/.agents/skills`）。
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

    /// 物化单个 Skill 文件为 [`PromptFragment`]。
    ///
    /// 用于把 typed 读取 / 解析错误作为 *值* 暴露给调用方与测试。读取失败
    /// 返回 [`SkillError::ReadFailed`]；frontmatter 缺失 / 未闭合 / YAML
    /// 非法返回 [`SkillError::ParseFailed`]。
    pub async fn materialize_one(
        path: &Path,
        kind: SkillSourceKind,
    ) -> Result<PromptFragment, SkillError> {
        let raw = parse_skill_file(path, kind)?;
        Ok(raw.into_fragment())
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
    /// `strict` 控制对已扫描文件读取 / 解析错误的处理：
    /// - `true`：遇到第一个 typed 错误立即返回 `Err`（用于
    ///   `materialize_available`，不静默跳过）；
    /// - `false`：记录 warn 日志并跳过该文件（用于 `list`，catalog 端口
    ///   签名无法返回 Err）。
    fn collect_available(
        &self,
        query: &SkillQuery,
        strict: bool,
    ) -> Result<Vec<RawSkill>, SkillError> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut acc: Vec<RawSkill> = Vec::new();
        for res in self.discover_all(query) {
            match res {
                Ok(raw) => {
                    if seen.insert(raw.name.clone()) {
                        acc.push(raw);
                    }
                }
                Err(err) => {
                    if strict {
                        return Err(err);
                    }
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "skill discovery skipped a file: {err}"
                    );
                }
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
        Ok(filtered)
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
        // catalog 端口签名无法返回 typed Err：此处采用 best-effort（lenient），
        // 记录并跳过单个文件的读取 / 解析错误。需要严格错误传播的调用方应
        // 使用 SkillMaterializationPort::materialize_available。
        self.collect_available(&query, false)
            .unwrap_or_default()
            .into_iter()
            .map(|raw| raw.into_descriptor())
            .collect()
    }
}

#[async_trait]
impl SkillMaterializationPort for FilesystemSkillAdapter {
    async fn materialize_available(
        &self,
        query: SkillMaterializationQuery,
    ) -> Result<SkillMaterializationSnapshot, SkillError> {
        // 把物化查询投影为 catalog 查询形状，复用发现 / 去重 / 过滤逻辑。
        let catalog_query = SkillQuery {
            project_root: query.project_root.clone(),
            extra_dirs: query.extra_dirs.clone(),
            available_tools: query.available_tools.clone(),
        };
        // strict=true：已扫描文件的读取 / 解析错误返回第一个 typed Err，
        // 不静默跳过。
        let raws = self.collect_available(&catalog_query, true)?;
        let fragments: Vec<PromptFragment> =
            raws.into_iter().map(|raw| raw.into_fragment()).collect();
        // fragments 已按 stable_key 排序；revision 是确定性内容 revision。
        let revision = SkillMaterializationRevision::from_fragments(&fragments);
        Ok(SkillMaterializationSnapshot::new(fragments, revision))
    }
}

// ── 内部：原始解析结果 ─────────────────────────────────────────────────

/// 单个 Skill 文件解析后的完整原始数据：descriptor 字段 + 物化正文 +
/// frontmatter 过滤声明。
#[derive(Debug, Clone)]
struct RawSkill {
    name: String,
    description: String,
    aliases: Vec<String>,
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
        SkillDescriptor::new(self.name, self.description, self.source, self.aliases)
    }

    fn into_fragment(self) -> PromptFragment {
        PromptFragment::new(self.name, self.content, self.source, CacheHint::Stable)
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

/// YAML frontmatter 中性结构（忽略未知字段）。
///
/// 恢复 `requires_tools` / `fallback_for`：前者声明该 Skill 依赖的工具，
/// 后者声明本 Skill 是哪些主 Skill 的 fallback。adapter 据此过滤。
#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    /// 所需工具名；任一缺失则该 Skill 不可见。
    #[serde(default)]
    requires_tools: Vec<String>,
    /// 本 Skill 是哪些（完整名）Skill 的 fallback；主 Skill 存在则隐藏。
    #[serde(default)]
    fallback_for: Vec<String>,
}

// ── 内部：发现（目录遍历） ─────────────────────────────────────────────

/// 扫描一个根目录：顶层 `.md` 文件直接解析；子目录若含 `skills/` 子目录
/// 则按 skill package 命名空间扫描，否则按普通 skill 目录扫描。
///
/// **目录不存在视为空（非错误）**；只有已扫描到的文件读取 / 解析失败才
/// 产生 typed [`SkillError`]。
fn scan_dir(dir: &Path, kind: SkillSourceKind, out: &mut Vec<Result<RawSkill, SkillError>>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // 目录不存在视为空（非错误）
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            out.push(parse_skill_file(&path, kind).map(|raw| apply_namespace(raw, None)));
        } else if path.is_dir() {
            let skills_child = path.join("skills");
            if skills_child.is_dir() {
                // skill package：<pkg>/skills/... → 命名空间前缀
                let pkg = path.file_name().map(|n| n.to_string_lossy().to_string());
                scan_subdir(&skills_child, kind, pkg.as_deref(), out);
            } else {
                // 普通 skill 目录：<dir>/<name>/SKILL.md
                scan_subdir(&path, kind, None, out);
            }
        }
    }
}

/// 扫描目录的直接 `.md` 文件与一层子目录中的 `.md` 文件；可选命名空间。
fn scan_subdir(
    dir: &Path,
    kind: SkillSourceKind,
    namespace: Option<&str>,
    out: &mut Vec<Result<RawSkill, SkillError>>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            out.push(parse_skill_file(&path, kind).map(|raw| apply_namespace(raw, namespace)));
        } else if path.is_dir() {
            // 再向下一层：扫描子-子目录中的 `.md` 文件。
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.extension().is_some_and(|e| e == "md") {
                        out.push(
                            parse_skill_file(&sub_path, kind)
                                .map(|raw| apply_namespace(raw, namespace)),
                        );
                    }
                }
            }
        }
    }
}

/// 应用命名空间前缀（skill package）。原 name 进入 aliases。
fn apply_namespace(mut raw: RawSkill, namespace: Option<&str>) -> RawSkill {
    if let Some(ns) = namespace {
        if !ns.is_empty() {
            raw.aliases.push(raw.name.clone());
            raw.name = format!("{ns}:{}", raw.name);
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
    let rest = &text[3..];
    let end = rest
        .find("---")
        .ok_or("YAML frontmatter 未闭合（缺少结束 `---`）")?;
    Ok(rest[..end].trim())
}

/// 抽取 frontmatter 之后的 markdown 正文。
fn extract_body(text: &str) -> String {
    if !text.starts_with("---") {
        return text.to_string();
    }
    let rest = &text[3..];
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
