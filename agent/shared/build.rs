//! build.rs: 两个职责
//! 1. 从 git tag 注入版本号到编译期常量（`cargo:rustc-env=AEMEATH_VERSION`）
//! 2. 用 syn 解析 `src/tool/types/*.rs` 中的 struct 定义，生成 ToolSchema impl

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

/// 将 Rust 类型映射到 JSON Schema 片段。
fn rust_type_to_json_schema(ty: &syn::Type, known_types: &HashSet<String>) -> String {
    match ty {
        syn::Type::Path(type_path) => {
            let segments: Vec<String> = type_path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            let name = segments.last().unwrap().as_str();

            match name {
                "String" | "PathBuf" | "OsString" => r#"{"type": "string"}"#.to_string(),
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                | "i128" | "isize" => r#"{"type": "integer"}"#.to_string(),
                "f32" | "f64" => r#"{"type": "number"}"#.to_string(),
                "bool" => r#"{"type": "boolean"}"#.to_string(),
                "Value" => r#"{}"#.to_string(),
                "Option" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            let inner = rust_type_to_json_schema(inner_ty, known_types);
                            return inner.replacen('}', r#","nullable": true}"#, 1);
                        }
                    }
                    r#"{"type": "object"}"#.to_string()
                }
                "Vec" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            let items = rust_type_to_json_schema(inner_ty, known_types);
                            return format!(r#"{{"type": "array", "items": {items}}}"#);
                        }
                    }
                    r#"{"type": "array"}"#.to_string()
                }
                "HashMap" | "BTreeMap" => r#"{"type": "object"}"#.to_string(),
                "Box" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            return rust_type_to_json_schema(inner_ty, known_types);
                        }
                    }
                    r#"{"type": "object"}"#.to_string()
                }
                other if known_types.contains(other) => r#"{"type": "object"}"#.to_string(),
                other => {
                    println!(
                        "cargo:warning=build.rs: 类型 `{other}` 未在 tool types 模块中定义，\
                         降级为 object。如需精确 schema，请将定义移入 tool types。"
                    );
                    r#"{"type": "object"}"#.to_string()
                }
            }
        }
        syn::Type::Reference(type_ref) => rust_type_to_json_schema(&type_ref.elem, known_types),
        _ => {
            panic!("build.rs: 无法解析的类型 {:?} in tool type struct.", ty);
        }
    }
}

/// 从 struct 定义提取字段，生成 ToolSchema impl。
fn generate_tool_schema_impl(
    item: &syn::ItemStruct,
    known_types: &HashSet<String>,
) -> Option<String> {
    let struct_name = item.ident.to_string();
    let mut properties = Vec::new();
    let mut required = Vec::new();

    for field in &item.fields {
        let field_name = match &field.ident {
            Some(name) => name.to_string(),
            None => continue,
        };

        let is_option = is_option_type(&field.ty);
        let schema = rust_type_to_json_schema(&field.ty, known_types);

        properties.push(format!("            \"{field_name}\": {schema}"));
        if !is_option {
            required.push(format!("            \"{field_name}\""));
        }
    }

    let properties_str = properties.join(",\n");
    let required_str = required.join(",\n");
    let sp = "    ";

    let schema = if required.is_empty() {
        format!(
            "{{\n\
             {sp}\"type\": \"object\",\n\
             {sp}\"properties\": {{\n\
             {properties_str}\n\
             {sp}}}\n\
             }}"
        )
    } else {
        format!(
            "{{\n\
             {sp}\"type\": \"object\",\n\
             {sp}\"properties\": {{\n\
             {properties_str}\n\
             {sp}}},\n\
             {sp}\"required\": [\n\
             {required_str}\n\
             {sp}]\n\
             }}"
        )
    };

    Some(format!(
        "\nimpl ToolSchema for {struct_name} {{\n\
         {sp}fn data_schema() -> Value {{\n\
         {sp}{sp}serde_json::json!({schema})\n\
         {sp}}}\n\
         }}\n"
    ))
}

fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

fn main() {
    // --- 1. 版本注入 ---
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let output = std::process::Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output();

    let tag = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };
    if !tag.is_empty() {
        let version = tag.strip_prefix(['v', 'V']).unwrap_or(&tag);
        println!("cargo:rustc-env=AEMEATH_VERSION={version}");
    }

    // --- 2. ToolSchema 代码生成（syn 解析） ---
    let out_dir = env::var("OUT_DIR").unwrap();
    let types_dir = Path::new("src/tool/types");

    if !types_dir.exists() {
        return;
    }

    println!("cargo:rerun-if-changed=src/tool/types");

    let mut impls = Vec::new();
    let skip_files = ["mod.rs"];

    // 第一遍：收集所有 struct 名
    let mut known_types = HashSet::new();
    let mut all_files = Vec::new();

    for entry in fs::read_dir(types_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let filename = path.file_name().unwrap().to_str().unwrap();
        if skip_files.contains(&filename) {
            continue;
        }

        let content = fs::read_to_string(&path).unwrap();
        let syn_file: syn::File = syn::parse_str(&content).unwrap_or_else(|e| {
            panic!("build.rs: syn 解析 {} 失败: {e}", path.display());
        });

        for item in &syn_file.items {
            if let syn::Item::Struct(item_struct) = item {
                if matches!(item_struct.vis, syn::Visibility::Public(_)) {
                    known_types.insert(item_struct.ident.to_string());
                }
            }
        }

        all_files.push((path.clone(), syn_file));
    }

    // 第二遍：为 Result struct 生成 ToolSchema impl
    for (path, syn_file) in &all_files {
        let filename = path.file_name().unwrap().to_str().unwrap();
        if filename == "support.rs" {
            continue;
        }

        for item in &syn_file.items {
            if let syn::Item::Struct(item_struct) = item {
                if !matches!(item_struct.vis, syn::Visibility::Public(_)) {
                    continue;
                }
                if let Some(impl_code) = generate_tool_schema_impl(item_struct, &known_types) {
                    impls.push(impl_code);
                }
            }
        }
    }

    let output_path = Path::new(&out_dir).join("generated_impls.rs");
    let mut output = String::new();
    output.push_str("// Generated by build.rs (syn parsing) — do not edit manually.\n");
    output.push_str("use serde_json::Value;\n");
    for impl_code in &impls {
        output.push_str(impl_code);
    }
    fs::write(&output_path, output).unwrap();
}
