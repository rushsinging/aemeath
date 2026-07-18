use std::path::Path;
use syn::{Attribute, Item, Visibility};

pub fn find_test_only_api_violations(path: &Path, source: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(source) else {
        return vec![format!("{}: Rust 语法解析失败", path.display())];
    };
    if has_cfg_test(&file.attrs) {
        return Vec::new();
    }
    let mut violations = Vec::new();
    visit_items(&file.items, false, &mut |item, in_test| {
        if in_test {
            return;
        }
        if let Item::Mod(module) = item {
            let name = module.ident.to_string();
            if matches!(
                name.as_str(),
                "testing" | "fixture" | "fixtures" | "fake" | "fakes"
            ) {
                violations.push(format!("{}: {name}", path.display()));
                return;
            }
        }
        if let Some((visibility, name)) = item_visibility_and_name(item) {
            if !matches!(visibility, Visibility::Inherited)
                && (name.contains("_for_test") || name.contains("test_only"))
            {
                violations.push(format!("{}: {name}", path.display()));
            }
        }
    });
    violations
}

pub fn production_dead_code_allow_count_for_path(path: &Path, source: &str) -> usize {
    if is_test_only_path(path) {
        return 0;
    }
    production_dead_code_allow_count(source)
}

pub fn production_dead_code_allow_count(source: &str) -> usize {
    let Ok(file) = syn::parse_file(source) else {
        return 0;
    };
    let mut count = 0;
    visit_items(&file.items, false, &mut |item, in_test| {
        if !in_test && has_dead_code_allow(item_attrs(item)) {
            count += 1;
        }
    });
    count
}

pub fn public_surface(path: &Path, source: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut surface = Vec::new();
    visit_items(&file.items, false, &mut |item, in_test| {
        if in_test {
            return;
        }
        if let Some((visibility, name)) = item_visibility_and_name(item) {
            if matches!(visibility, Visibility::Public(_)) {
                let kind = item_kind(item);
                surface.push(format!("{}: pub {kind} {name}", path.display()));
            }
        }
        if let Item::Impl(implementation) = item {
            let owner = implementation.self_ty.to_token_stream().to_string();
            for member in &implementation.items {
                if let syn::ImplItem::Fn(function) = member {
                    if matches!(function.vis, Visibility::Public(_))
                        && !has_cfg_test(&function.attrs)
                    {
                        surface.push(format!(
                            "{}: pub fn {owner}::{}",
                            path.display(),
                            function.sig.ident
                        ));
                    }
                }
            }
        }
    });
    surface.sort();
    surface
}

fn visit_items(items: &[Item], parent_test: bool, visitor: &mut impl FnMut(&Item, bool)) {
    for item in items {
        let in_test = parent_test || has_cfg_test(item_attrs(item));
        visitor(item, in_test);
        if let Item::Mod(module) = item {
            if let Some((_, nested)) = &module.content {
                visit_items(nested, in_test, visitor);
            }
        }
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(v) => &v.attrs,
        Item::Enum(v) => &v.attrs,
        Item::Fn(v) => &v.attrs,
        Item::Mod(v) => &v.attrs,
        Item::Static(v) => &v.attrs,
        Item::Struct(v) => &v.attrs,
        Item::Trait(v) => &v.attrs,
        Item::Type(v) => &v.attrs,
        Item::Union(v) => &v.attrs,
        _ => &[],
    }
}

fn item_visibility_and_name(item: &Item) -> Option<(&Visibility, String)> {
    match item {
        Item::Const(v) => Some((&v.vis, v.ident.to_string())),
        Item::Enum(v) => Some((&v.vis, v.ident.to_string())),
        Item::Fn(v) => Some((&v.vis, v.sig.ident.to_string())),
        Item::Mod(v) => Some((&v.vis, v.ident.to_string())),
        Item::Static(v) => Some((&v.vis, v.ident.to_string())),
        Item::Struct(v) => Some((&v.vis, v.ident.to_string())),
        Item::Trait(v) => Some((&v.vis, v.ident.to_string())),
        Item::Type(v) => Some((&v.vis, v.ident.to_string())),
        Item::Union(v) => Some((&v.vis, v.ident.to_string())),
        _ => None,
    }
}

fn item_kind(item: &Item) -> &'static str {
    match item {
        Item::Const(_) => "const",
        Item::Enum(_) => "enum",
        Item::Fn(_) => "fn",
        Item::Mod(_) => "mod",
        Item::Static(_) => "static",
        Item::Struct(_) => "struct",
        Item::Trait(_) => "trait",
        Item::Type(_) => "type",
        Item::Union(_) => "union",
        _ => "item",
    }
}

fn is_test_only_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
        || path.file_stem().is_some_and(|stem| {
            let stem = stem.to_string_lossy();
            stem == "tests" || stem.ends_with("_test") || stem.ends_with("_tests")
        })
}

fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .is_ok_and(|meta| cfg_meta_contains_test(&meta))
    })
}

fn cfg_meta_contains_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let nested = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            );
            nested.is_ok_and(|items| items.iter().any(cfg_meta_contains_test))
        }
        syn::Meta::NameValue(_) => false,
    }
}

fn has_dead_code_allow(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("allow")
            && attr
                .meta
                .to_token_stream()
                .to_string()
                .contains("dead_code")
    })
}

use quote::ToTokens;
