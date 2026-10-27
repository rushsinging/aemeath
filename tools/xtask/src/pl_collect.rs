//! PL 符号采集器（`xtask pl-collect`）：为 published-language-map 设计图
//! 采集 crate 根导出符号的定义面与全仓消费点。
//!
//! 采集口径与 context 版设计图一致：
//! - 根导出面：crate `lib.rs` 的 `pub use`（含嵌套 `pub mod` 内）叶子符号；
//! - 定义面：符号在 crate 内的 kind、定义文件、签名行、字段、方法；
//! - 消费点：全仓 `*.rs` 的 `ExprCall`（调用）/`ExprStruct`（构造）/类型位置
//!   `TypePath`（类型引用）/`ItemImpl` trait 路径（接口实现）四类命中，
//!   `use`/`pub use` 整棵跳过（re-export 不误算消费），排除目标 crate 自身；
//! - `duty`/`deep_path`/分组/链路为人工编辑内容，采集输出留空占位。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use quote::ToTokens;
use serde::Serialize;
use syn::visit::Visit;
use syn::{File, Item, UseTree};

use crate::guards_engine::normalize_whitespace;

/// 设计图 payload 中单个符号的采集结果（人工字段留空占位）。
#[derive(Serialize)]
pub struct SymbolEntry {
    pub kind: String,
    pub duty: Option<String>,
    pub deep_path: Option<String>,
    pub def: String,
    pub signature: String,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
    pub consumer_count: usize,
    pub consumers: Vec<ConsumerEntry>,
}

/// 单文件消费者聚合（forms 去重后按首次出现排序）。
#[derive(Serialize)]
pub struct ConsumerEntry {
    pub file: String,
    pub module: String,
    pub forms: Vec<String>,
    pub snippets: Vec<SnippetEntry>,
}

/// 单个命中点：行号、该行原文、命中类别。
#[derive(Serialize)]
pub struct SnippetEntry {
    pub line: usize,
    pub code: String,
    pub kind: String,
    pub label: String,
}

/// 符号定义记录（合并自 crate 内多文件扫描）。
#[derive(Default)]
pub struct SymbolDefinitions {
    records: BTreeMap<String, DefinitionRecord>,
}

/// 单个符号的定义信息。
pub struct DefinitionRecord {
    pub kind: &'static str,
    pub def: String,
    pub signature: String,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
}

/// 提取 crate 根导出面：`lib.rs` 全部 `pub use` 叶子符号（含嵌套模块）。
pub fn extract_root_exports(lib_source: &str) -> Result<Vec<String>> {
    let file: File = syn::parse_file(lib_source).context("解析 lib.rs 失败")?;
    let mut exports = BTreeSet::new();
    collect_public_use_leaves(&file.items, &mut exports);
    Ok(exports.into_iter().collect())
}

/// 递归收集 items 中（含嵌套 `pub mod`）public `pub use` 的叶子符号名。
fn collect_public_use_leaves(items: &[Item], exports: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Item::Use(use_item) if matches!(use_item.vis, syn::Visibility::Public(_)) => {
                collect_use_tree_leaves(&use_item.tree, exports);
            }
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_public_use_leaves(nested, exports);
                }
            }
            _ => {}
        }
    }
}

/// 展开 use 树：`Name`/`Rename` 是叶子，`Path`/`Group` 递归，`Glob` 忽略。
fn collect_use_tree_leaves(tree: &UseTree, exports: &mut BTreeSet<String>) {
    match tree {
        UseTree::Name(name) => {
            exports.insert(name.ident.to_string());
        }
        UseTree::Rename(rename) => {
            exports.insert(rename.rename.to_string());
        }
        UseTree::Group(group) => {
            for nested in &group.items {
                collect_use_tree_leaves(nested, exports);
            }
        }
        UseTree::Path(segment) => collect_use_tree_leaves(&segment.tree, exports),
        UseTree::Glob(_) => {}
    }
}

impl SymbolDefinitions {
    /// 扫描单个源文件，返回其中命中的符号定义。
    /// `alias_map` 为 crate 内 `use 真名 as 别名` 映射（别名发布时按别名键记录）。
    pub fn scan(
        source: &str,
        def_file: &str,
        names: &BTreeSet<String>,
        alias_map: &BTreeMap<String, String>,
    ) -> SymbolDefinitions {
        let Ok(file) = syn::parse_file(source) else {
            return SymbolDefinitions::default();
        };
        let mut definitions = SymbolDefinitions::default();
        definitions.scan_items(&file.items, source, def_file, names, alias_map);
        definitions
    }

    /// 合并另一份扫描结果（同名定义保留先出现者）。
    fn merge(&mut self, other: SymbolDefinitions) {
        for (name, record) in other.records {
            self.records.entry(name).or_insert(record);
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&DefinitionRecord> {
        self.records.get(name)
    }

    fn scan_items(
        &mut self,
        items: &[Item],
        source: &str,
        def_file: &str,
        names: &BTreeSet<String>,
        alias_map: &BTreeMap<String, String>,
    ) {
        for item in items {
            match item {
                Item::Struct(item_struct) => {
                    if let Some(key) = record_key(&item_struct.ident.to_string(), names, alias_map)
                    {
                        let signature =
                            definition_line(source, &format!("pub struct {}", item_struct.ident));
                        let fields = struct_field_lines(item_struct);
                        self.records.insert(
                            key,
                            DefinitionRecord {
                                kind: "struct",
                                def: def_file.to_owned(),
                                signature,
                                fields,
                                methods: Vec::new(),
                            },
                        );
                    }
                }
                Item::Enum(item_enum) => {
                    if let Some(key) = record_key(&item_enum.ident.to_string(), names, alias_map) {
                        let signature =
                            definition_line(source, &format!("pub enum {}", item_enum.ident));
                        let fields = enum_variant_lines(item_enum);
                        self.records.insert(
                            key,
                            DefinitionRecord {
                                kind: "enum",
                                def: def_file.to_owned(),
                                signature,
                                fields,
                                methods: Vec::new(),
                            },
                        );
                    }
                }
                Item::Trait(item_trait) => {
                    if let Some(key) = record_key(&item_trait.ident.to_string(), names, alias_map) {
                        let signature =
                            definition_line(source, &format!("pub trait {}", item_trait.ident));
                        let methods = trait_method_lines(item_trait);
                        self.records.insert(
                            key,
                            DefinitionRecord {
                                kind: "trait",
                                def: def_file.to_owned(),
                                signature,
                                fields: Vec::new(),
                                methods,
                            },
                        );
                    }
                }
                Item::Fn(item_fn) => {
                    if let Some(key) = record_key(&item_fn.sig.ident.to_string(), names, alias_map)
                    {
                        let signature =
                            definition_line(source, &format!("pub fn {}", item_fn.sig.ident));
                        self.records.insert(
                            key,
                            DefinitionRecord {
                                kind: "fn",
                                def: def_file.to_owned(),
                                signature,
                                fields: Vec::new(),
                                methods: Vec::new(),
                            },
                        );
                    }
                }
                Item::Type(item_type) => {
                    if let Some(key) = record_key(&item_type.ident.to_string(), names, alias_map) {
                        let signature =
                            definition_line(source, &format!("pub type {}", item_type.ident));
                        self.records.insert(
                            key,
                            DefinitionRecord {
                                kind: "type",
                                def: def_file.to_owned(),
                                signature,
                                fields: Vec::new(),
                                methods: Vec::new(),
                            },
                        );
                    }
                }
                Item::Impl(item_impl) => {
                    if let Some(self_type_name) = impl_self_type_name(item_impl) {
                        if let Some(key) = record_key(&self_type_name, names, alias_map) {
                            let methods = impl_method_lines(item_impl);
                            if let Some(record) = self.records.get_mut(&key) {
                                record.methods.extend(methods);
                            }
                        }
                    }
                }
                Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        self.scan_items(nested, source, def_file, names, alias_map);
                    }
                }
                _ => {}
            }
        }
    }
}

/// 定义名 → 记录键：真名在根导出面内直接用；
/// 真名被 `use 真名 as 别名` 重命名且别名在根导出面内时用别名键。
fn record_key(
    definition_name: &str,
    names: &BTreeSet<String>,
    alias_map: &BTreeMap<String, String>,
) -> Option<String> {
    if names.contains(definition_name) {
        return Some(definition_name.to_owned());
    }
    if let Some(alias) = alias_map.get(definition_name) {
        if names.contains(alias) {
            return Some(alias.clone());
        }
    }
    None
}

/// 收集 crate 源文件内全部 `use 真名 as 别名`（不限 pub，含嵌套模块）。
pub fn collect_alias_map(source: &str) -> BTreeMap<String, String> {
    let mut alias_map = BTreeMap::new();
    if let Ok(file) = syn::parse_file(source) {
        collect_alias_items(&file.items, &mut alias_map);
    }
    alias_map
}

fn collect_alias_items(items: &[Item], alias_map: &mut BTreeMap<String, String>) {
    for item in items {
        match item {
            Item::Use(use_item) => collect_alias_tree(&use_item.tree, alias_map),
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_alias_items(nested, alias_map);
                }
            }
            _ => {}
        }
    }
}

fn collect_alias_tree(tree: &UseTree, alias_map: &mut BTreeMap<String, String>) {
    match tree {
        UseTree::Rename(rename) => {
            alias_map.insert(rename.ident.to_string(), rename.rename.to_string());
        }
        UseTree::Group(group) => {
            for nested in &group.items {
                collect_alias_tree(nested, alias_map);
            }
        }
        UseTree::Path(segment) => collect_alias_tree(&segment.tree, alias_map),
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

/// 从定义文本锚点定位源码行原文（规范化前缀匹配）。
fn definition_line(source: &str, anchor: &str) -> String {
    let normalized = normalize_whitespace(anchor);
    for line in source.lines() {
        if normalize_whitespace(line).starts_with(&normalized) {
            return line.trim_end().to_owned();
        }
    }
    anchor.to_owned()
}

/// struct 字段行（`pub model: String` 形式，token 重造）。
fn struct_field_lines(item_struct: &syn::ItemStruct) -> Vec<String> {
    item_struct
        .fields
        .iter()
        .map(|field| {
            let visibility = match &field.vis {
                syn::Visibility::Public(_) => "pub ".to_owned(),
                syn::Visibility::Restricted(_) => "pub(crate) ".to_owned(),
                syn::Visibility::Inherited => String::new(),
            };
            let name = field
                .ident
                .as_ref()
                .map(|ident| format!("{visibility}{ident}: "))
                .unwrap_or_default();
            format!("{name}{}", field.ty.to_token_stream())
        })
        .collect()
}

/// enum 变体行（含变体字段）。
fn enum_variant_lines(item_enum: &syn::ItemEnum) -> Vec<String> {
    item_enum
        .variants
        .iter()
        .map(|variant| {
            let mut line = variant.ident.to_string();
            if !variant.fields.is_empty() {
                let fields: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|field| {
                        field
                            .ident
                            .as_ref()
                            .map(|ident| format!("{ident}: "))
                            .unwrap_or_default()
                            + &field.ty.to_token_stream().to_string()
                    })
                    .collect();
                line.push_str(&format!(" {{ {} }}", fields.join(", ")));
            }
            line
        })
        .collect()
}

/// trait 方法签名行。
fn trait_method_lines(item_trait: &syn::ItemTrait) -> Vec<String> {
    item_trait
        .items
        .iter()
        .filter_map(|item| match item {
            syn::TraitItem::Fn(method) => Some(render_fn_signature(&method.sig, "fn")),
            _ => None,
        })
        .collect()
}

/// impl 块方法签名行。
fn impl_method_lines(item_impl: &syn::ItemImpl) -> Vec<String> {
    item_impl
        .items
        .iter()
        .filter_map(|item| match item {
            syn::ImplItem::Fn(method) => {
                let visibility = match &method.vis {
                    syn::Visibility::Public(_) => "pub fn",
                    _ => "fn",
                };
                Some(render_fn_signature(&method.sig, visibility))
            }
            _ => None,
        })
        .collect()
}

/// fn 签名行（`pub fn name(args) -> Ret`）。
fn render_fn_signature(signature: &syn::Signature, keyword: &str) -> String {
    let mut text = format!("{keyword} {}", signature.ident);
    let inputs: Vec<String> = signature
        .inputs
        .iter()
        .map(|input| input.to_token_stream().to_string())
        .collect();
    text.push_str(&format!("({})", inputs.join(", ")));
    if let syn::ReturnType::Type(_, return_type) = &signature.output {
        text.push_str(&format!(" -> {}", return_type.to_token_stream()));
    }
    text
}

/// impl 目标类型名（路径末段）。
fn impl_self_type_name(item_impl: &syn::ItemImpl) -> Option<String> {
    if let syn::Type::Path(type_path) = &*item_impl.self_ty {
        path_last_segment(&type_path.path).map(|segment| segment.to_string())
    } else {
        None
    }
}

/// 路径末段标识符。
fn path_last_segment(path: &syn::Path) -> Option<&syn::Ident> {
    path.segments.last().map(|segment| &segment.ident)
}

/// 单个消费点命中（含命中的符号名）。
struct SourceHit {
    symbol: String,
    line: usize,
    kind: &'static str,
    label: &'static str,
}

/// 消费点扫描：解析单文件源码，返回「符号 → 该文件消费者条目」。
/// 无命中返回空 map；`use`/`pub use` 整棵跳过。
pub fn scan_source(
    source: &str,
    names: &BTreeSet<String>,
    file_rel: &str,
) -> BTreeMap<String, ConsumerEntry> {
    let Ok(file) = syn::parse_file(source) else {
        return BTreeMap::new();
    };
    let mut visitor = ConsumerVisitor {
        names,
        lines: source.lines().map(|line| line.trim_end()).collect(),
        cursor: 0,
        hits: Vec::new(),
    };
    visitor.visit_file(&file);
    let mut result: BTreeMap<String, Vec<SnippetEntry>> = BTreeMap::new();
    for hit in &visitor.hits {
        result
            .entry(hit.symbol.clone())
            .or_default()
            .push(SnippetEntry {
                line: hit.line,
                code: visitor.lines[hit.line - 1].trim().to_owned(),
                kind: hit.kind.to_owned(),
                label: hit.label.to_owned(),
            });
    }
    result
        .into_iter()
        .map(|(symbol, mut snippets)| {
            snippets.sort_by_key(|snippet| (snippet.line, snippet.kind.clone()));
            snippets.dedup_by_key(|snippet| (snippet.line, snippet.kind.clone()));
            let mut forms: Vec<String> = snippets
                .iter()
                .map(|snippet| snippet.label.clone())
                .collect();
            forms.dedup();
            (
                symbol,
                ConsumerEntry {
                    file: file_rel.to_owned(),
                    module: module_label(file_rel),
                    forms,
                    snippets,
                },
            )
        })
        .collect()
}

/// 文件所属 crate 标注（`runtime :: application/x.rs` 形式）。
fn module_label(file_rel: &str) -> String {
    let segments: Vec<&str> = file_rel.split('/').collect();
    let unknown = ("unknown".to_owned(), segments.join("/"));
    let (crate_name, inner) = if segments.first() == Some(&"agent") {
        match (segments.get(1), segments.get(2), segments.get(3)) {
            (Some(&"features"), Some(&feature), Some(&"src")) => {
                (feature.to_owned(), segments[4..].join("/"))
            }
            (Some(&"features"), Some(&feature), _) => (feature.to_owned(), segments[3..].join("/")),
            (Some(&"composition"), Some(_), Some(&"src")) => {
                ("composition".to_owned(), segments[4..].join("/"))
            }
            (Some(&"composition"), Some(_), _) => {
                ("composition".to_owned(), segments[3..].join("/"))
            }
            (Some(&"shared"), Some(_), Some(&"src")) => {
                ("share".to_owned(), segments[4..].join("/"))
            }
            (Some(&"shared"), Some(_), _) => ("share".to_owned(), segments[3..].join("/")),
            _ => unknown,
        }
    } else if segments.first() == Some(&"packages") {
        match (segments.get(1), segments.get(2)) {
            (Some(&"sdk"), Some(&"src")) => ("sdk".to_owned(), segments[3..].join("/")),
            (Some(&"global"), Some(&area)) => {
                let crate_name = area.to_owned();
                match segments.get(3) {
                    Some(&"src") => (crate_name, segments[4..].join("/")),
                    _ => (crate_name, segments[3..].join("/")),
                }
            }
            _ => unknown,
        }
    } else if segments.first() == Some(&"apps") || segments.first() == Some(&"tools") {
        match (segments.get(1), segments.get(2)) {
            (Some(&area), Some(&"src")) => (area.to_owned(), segments[3..].join("/")),
            (Some(&area), _) => (area.to_owned(), segments[2..].join("/")),
            _ => unknown,
        }
    } else {
        unknown
    };
    format!("{crate_name} :: {inner}")
}

/// 消费点 AST 访问器：游标式行定位（AST 遍历序与源码序基本一致）。
struct ConsumerVisitor<'a> {
    names: &'a BTreeSet<String>,
    lines: Vec<&'a str>,
    cursor: usize,
    hits: Vec<SourceHit>,
}

impl ConsumerVisitor<'_> {
    /// 从游标位置起找包含锚点的首个行号；`use` 行不作为定位目标
    /// （AST 层跳过 use，文本定位也要一致），找不到回退当前游标行。
    fn locate(&mut self, anchor: &str) -> usize {
        let mut index = self.cursor;
        while index < self.lines.len() {
            let trimmed = self.lines[index].trim_start();
            if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
                index += 1;
                continue;
            }
            if normalize_whitespace(self.lines[index]).contains(anchor) {
                self.cursor = index;
                return index + 1;
            }
            index += 1;
        }
        self.cursor.saturating_sub(1) + 1
    }

    fn record(&mut self, symbol: &str, anchor: &str, kind: &'static str, label: &'static str) {
        let line = self.locate(anchor);
        if !self
            .hits
            .iter()
            .any(|hit| hit.symbol == symbol && hit.line == line && hit.kind == kind)
        {
            self.hits.push(SourceHit {
                symbol: symbol.to_owned(),
                line,
                kind,
                label,
            });
        }
    }
}

impl<'a> Visit<'a> for ConsumerVisitor<'a> {
    /// `use`/`pub use` 整棵跳过：re-export 不是消费。
    fn visit_item_use(&mut self, _use_item: &'a syn::ItemUse) {}

    fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*call.func {
            for symbol in path_hit_symbols(&expr_path.path, self.names) {
                let anchor = format!("{}(", path_text(&expr_path.path));
                self.record(&symbol, &anchor, "call", "调用");
            }
        }
        // callee 路径已按调用记录，子访问跳过 func 只走实参。
        for argument in &call.args {
            self.visit_expr(argument);
        }
    }

    fn visit_expr_struct(&mut self, expr_struct: &'a syn::ExprStruct) {
        if let Some(segment) = path_last_segment(&expr_struct.path) {
            let symbol = segment.to_string();
            if self.names.contains(&symbol) {
                let anchor = path_text(&expr_struct.path);
                self.record(&symbol, &anchor, "ctor", "构造");
            }
        }
        // 路径已按构造记录，子访问只走字段与 rest。
        for field in &expr_struct.fields {
            self.visit_expr(&field.expr);
        }
        if let Some(rest) = &expr_struct.rest {
            self.visit_expr(rest);
        }
    }

    fn visit_expr_path(&mut self, expr_path: &'a syn::ExprPath) {
        // 裸路径引用（枚举变体 match 臂、函数传参等）：任一段命中即记。
        for symbol in path_hit_symbols(&expr_path.path, self.names) {
            let anchor = path_text(&expr_path.path);
            self.record(&symbol, &anchor, "ref", "路径引用");
        }
        syn::visit::visit_expr_path(self, expr_path);
    }

    fn visit_type_path(&mut self, type_path: &'a syn::TypePath) {
        if let Some(segment) = path_last_segment(&type_path.path) {
            let symbol = segment.to_string();
            if self.names.contains(&symbol) {
                let anchor = path_text(&type_path.path);
                self.record(&symbol, &anchor, "fnty", "类型引用");
            }
        }
        syn::visit::visit_type_path(self, type_path);
    }

    fn visit_trait_bound(&mut self, bound: &'a syn::TraitBound) {
        if let Some(segment) = path_last_segment(&bound.path) {
            let symbol = segment.to_string();
            if self.names.contains(&symbol) {
                let anchor = path_text(&bound.path);
                self.record(&symbol, &anchor, "fnty", "类型引用");
            }
        }
        syn::visit::visit_trait_bound(self, bound);
    }

    fn visit_item_impl(&mut self, item_impl: &'a syn::ItemImpl) {
        if let Some((_, trait_path, _)) = &item_impl.trait_ {
            if let Some(segment) = path_last_segment(trait_path) {
                let symbol = segment.to_string();
                if self.names.contains(&symbol) {
                    let anchor = format!("{} for", path_text(trait_path));
                    self.record(&symbol, &anchor, "implty", "接口实现");
                }
            }
        }
        syn::visit::visit_item_impl(self, item_impl);
    }
}

/// 路径各段命中的目标符号集合（覆盖关联调用 `Name::method()`）。
fn path_hit_symbols(path: &syn::Path, names: &BTreeSet<String>) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .filter(|ident| names.contains(ident))
        .collect()
}

/// 路径完整文本（`a::b::C`）。
fn path_text(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// 采集入口：crate 目录 + 仓库根 → 设计图 symbol payload（JSON 文本）。
pub fn collect(crate_dir: &Path, repo_root: &Path) -> Result<String> {
    let lib_path = crate_dir.join("src/lib.rs");
    let lib_source = fs::read_to_string(&lib_path)
        .with_context(|| format!("读取 {} 失败", lib_path.display()))?;
    let exports = extract_root_exports(&lib_source)?;
    let names: BTreeSet<String> = exports.into_iter().collect();

    let mut definitions = SymbolDefinitions::default();
    for source_file in rust_files_under(&crate_dir.join("src"))? {
        let source = fs::read_to_string(&source_file)
            .with_context(|| format!("读取 {} 失败", source_file.display()))?;
        let alias_map = collect_alias_map(&source);
        let relative = relative_path(&source_file, repo_root);
        definitions.merge(SymbolDefinitions::scan(
            &source, &relative, &names, &alias_map,
        ));
    }

    let crate_prefix = format!("{}/", relative_path(crate_dir, repo_root));
    let mut consumers_by_symbol: BTreeMap<String, Vec<ConsumerEntry>> = BTreeMap::new();
    for source_file in rust_files_under(repo_root)? {
        let relative = relative_path(&source_file, repo_root);
        if relative.starts_with(&crate_prefix) {
            continue;
        }
        let source = fs::read_to_string(&source_file)
            .with_context(|| format!("读取 {} 失败", source_file.display()))?;
        for (symbol, entry) in scan_source(&source, &names, &relative) {
            consumers_by_symbol.entry(symbol).or_default().push(entry);
        }
    }

    let mut symbols: BTreeMap<String, SymbolEntry> = BTreeMap::new();
    for name in &names {
        let consumers = consumers_by_symbol.remove(name).unwrap_or_default();
        let consumer_count = consumers.len();
        let entry = match definitions.get(name) {
            Some(record) => SymbolEntry {
                kind: record.kind.to_owned(),
                duty: None,
                deep_path: None,
                def: record.def.clone(),
                signature: record.signature.clone(),
                fields: record.fields.clone(),
                methods: record.methods.clone(),
                consumer_count,
                consumers,
            },
            // 根导出面里的 pub mod（如 test_support）无本 crate 类型定义。
            None => SymbolEntry {
                kind: "module".to_owned(),
                duty: None,
                deep_path: None,
                def: String::new(),
                signature: String::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                consumer_count,
                consumers,
            },
        };
        symbols.insert(name.clone(), entry);
    }
    serde_json::to_string_pretty(&symbols).context("序列化 payload 失败")
}

/// 递归收集目录下全部 `*.rs`（跳过 target/.git/node_modules）。
fn rust_files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit_rust_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn visit_rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录 {} 失败", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("遍历 {} 失败", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), "target" | ".git" | "node_modules") {
                continue;
            }
            visit_rust_files(&path, files)?;
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// 相对仓库根的路径（`/` 分隔）。
fn relative_path(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
