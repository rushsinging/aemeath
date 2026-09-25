use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// 单条 `use` 语句展开后的完整路径（含来源行号与测试上下文标记）。
#[derive(Debug, Clone)]
pub struct UsePath {
    pub text: String,
    pub line: usize,
    pub in_test_module: bool,
}

/// 单个源文件的结构索引：use 树展开 + crate 根 `pub use` 导出面。
#[derive(Debug, Default)]
pub struct FileIndex {
    pub use_paths: Vec<UsePath>,
    pub public_reexports: Vec<String>,
}

impl FileIndex {
    /// 生产区 use 路径（排除 `#[cfg(test)]` 模块内的 use）。
    pub fn production_use_paths(&self) -> Vec<&UsePath> {
        self.use_paths
            .iter()
            .filter(|path| !path.in_test_module)
            .collect()
    }
}

/// 解析单个源文件：syn 语法级展开 use 树，标记 cfg(test) 上下文。
pub fn index_file(source_path: &Path) -> Result<FileIndex> {
    let source = fs::read_to_string(source_path)
        .with_context(|| format!("读取 {} 失败", source_path.display()))?;
    let syntax =
        syn::parse_file(&source).with_context(|| format!("解析 {} 失败", source_path.display()))?;
    let mut index = FileIndex::default();
    let mut collector = UseCollector {
        source: &source,
        index: &mut index,
        in_test_module: false,
    };
    collector.visit_items(&syntax.items);
    Ok(index)
}

/// 递归索引目录下全部 `.rs` 文件（跳过隐藏目录与 target）。
pub fn index_directory(directory: &Path) -> Result<Vec<FileIndex>> {
    let mut indexes = Vec::new();
    collect_rust_files(directory, &mut indexes)?;
    Ok(indexes)
}

fn collect_rust_files(dir: &Path, indexes: &mut Vec<FileIndex>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录 {} 失败", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_rust_files(&path, indexes)?;
        } else if name.ends_with(".rs") {
            indexes.push(index_file(&path)?);
        }
    }
    Ok(())
}

struct UseCollector<'a> {
    source: &'a str,
    index: &'a mut FileIndex,
    in_test_module: bool,
}

impl<'a> UseCollector<'a> {
    fn visit_items(&mut self, items: &[syn::Item]) {
        for item in items {
            match item {
                syn::Item::Use(use_item) => self.visit_use(use_item),
                syn::Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        let was_in_test = self.in_test_module;
                        self.in_test_module = self.in_test_module || is_cfg_test(&module.attrs);
                        self.visit_items(nested);
                        self.in_test_module = was_in_test;
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_use(&mut self, use_item: &syn::ItemUse) {
        let statement = render_use_statement(use_item);
        let line = line_of(self.source, &statement);
        let is_public = matches!(use_item.vis, syn::Visibility::Public(_));
        let leading = if use_item.leading_colon.is_some() {
            "::"
        } else {
            ""
        };
        expand_tree(&use_item.tree, leading, &mut |full_path, leaf| {
            self.index.use_paths.push(UsePath {
                text: full_path,
                line,
                in_test_module: self.in_test_module,
            });
            if is_public {
                self.index.public_reexports.push(leaf);
            }
        });
    }
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg") && quote::quote!(#attr).to_string().contains("test")
    })
}

fn expand_tree(tree: &syn::UseTree, prefix: &str, emit: &mut impl FnMut(String, String)) {
    match tree {
        syn::UseTree::Path(segment) => {
            let path = join_path(prefix, &segment.ident.to_string());
            expand_tree(&segment.tree, &path, emit);
        }
        syn::UseTree::Name(name) => {
            let leaf = name.ident.to_string();
            emit(join_path(prefix, &leaf), leaf);
        }
        syn::UseTree::Rename(rename) => {
            let alias = rename.rename.to_string();
            emit(join_path(prefix, &rename.ident.to_string()), alias);
        }
        syn::UseTree::Glob(_) => {
            emit(format!("{prefix}*"), "*".to_owned());
        }
        syn::UseTree::Group(group) => {
            for nested in &group.items {
                expand_tree(nested, prefix, emit);
            }
        }
    }
}

fn join_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() || prefix == "::" {
        format!("{prefix}{segment}")
    } else {
        format!("{prefix}::{segment}")
    }
}

fn render_use_statement(use_item: &syn::ItemUse) -> String {
    let mut text = String::new();
    if matches!(use_item.vis, syn::Visibility::Public(_)) {
        text.push_str("pub ");
    }
    text.push_str("use ");
    if use_item.leading_colon.is_some() {
        text.push_str("::");
    }
    text.push_str(&render_tree(&use_item.tree));
    text.push(';');
    text
}

fn render_tree(tree: &syn::UseTree) -> String {
    match tree {
        syn::UseTree::Path(segment) => {
            format!("{}::{}", segment.ident, render_tree(&segment.tree))
        }
        syn::UseTree::Name(name) => name.ident.to_string(),
        syn::UseTree::Rename(rename) => format!("{} as {}", rename.ident, rename.rename),
        syn::UseTree::Glob(_) => "*".to_owned(),
        syn::UseTree::Group(group) => {
            let branches: Vec<String> = group.items.iter().map(render_tree).collect();
            format!("{{{}}}", branches.join(", "))
        }
    }
}

/// 语句在源码中的行号：先按归一化后整行匹配（覆盖单行 use），
/// 再退化为语句前缀匹配（覆盖多行 group use 的首行）。
fn line_of(source: &str, statement: &str) -> usize {
    let normalized = normalize_whitespace(statement);
    for (index, line) in source.lines().enumerate() {
        if normalize_whitespace(line) == normalized {
            return index + 1;
        }
    }
    let prefix: String = normalized.chars().take(48).collect();
    if prefix.is_empty() {
        return 1;
    }
    for (index, line) in source.lines().enumerate() {
        if normalize_whitespace(line).starts_with(&prefix) {
            return index + 1;
        }
    }
    1
}

fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}
