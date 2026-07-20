//! build.rs: 一个职责
//!
//! 用 syn 解析 `src/domain/types/*.rs` 中的 struct 定义，生成 ToolSchema impl。
//!
//! 版本号注入说明：build.rs 不再从 git tag 读取版本号，而是尊重外部传入的
//! `AEMEATH_VERSION` 环境变量（由打包/CI 显式注入），无外部注入时 fallback 到
//! `CARGO_PKG_VERSION`（即 `Cargo.toml` 的 `workspace.version` 占位符 `0.0.0`）。
//! 本地 dev build 永远显示 `0.0.0`，发布版本由 release workflow 注入。

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

/// 将 Rust 类型映射到 JSON Schema 片段。
fn rust_type_to_json_schema(
    ty: &syn::Type,
    known_structs: &HashMap<String, syn::ItemStruct>,
    known_enums: &HashMap<String, Vec<String>>,
) -> String {
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
                "String" | "PathBuf" | "OsString" | "TaskId" => r#"{"type": "string"}"#.to_string(),
                "BatchId" => r#"{"type": "integer"}"#.to_string(),
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
                            let inner =
                                rust_type_to_json_schema(inner_ty, known_structs, known_enums);
                            // 在**最外层** `}` 前插入 nullable：嵌套对象（如 Vec<Value> 的
                            // `{"type":"array","items":{}}`）的第一个 `}` 是内层的，不能用
                            // replacen 第一个；空对象 `{}` 特判避免产生首逗号。
                            if inner.trim() == "{}" {
                                return r#"{ "nullable": true }"#.to_string();
                            }
                            if let Some(pos) = inner.rfind('}') {
                                let mut s = inner.clone();
                                s.insert_str(pos, r#","nullable": true"#);
                                return s;
                            }
                            return inner;
                        }
                    }
                    r#"{"type": "object"}"#.to_string()
                }
                "Vec" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            let items =
                                rust_type_to_json_schema(inner_ty, known_structs, known_enums);
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
                            return rust_type_to_json_schema(inner_ty, known_structs, known_enums);
                        }
                    }
                    r#"{"type": "object"}"#.to_string()
                }
                other if known_enums.contains_key(other) => {
                    let variants = known_enums.get(other).unwrap();
                    let enum_values: Vec<String> =
                        variants.iter().map(|v| format!(r#""{v}""#)).collect();
                    format!(
                        r#"{{"type": "string", "enum": [{}]}}"#,
                        enum_values.join(", ")
                    )
                }
                other if known_structs.contains_key(other) => {
                    let item = known_structs.get(other).expect("known struct must exist");
                    generate_json_schema(item, known_structs, known_enums)
                }
                other => {
                    println!(
                        "cargo:warning=build.rs: 类型 `{other}` 未在 tool types 模块中定义，\
                         降级为 object。如需精确 schema，请将定义移入 tool types。"
                    );
                    r#"{"type": "object"}"#.to_string()
                }
            }
        }
        syn::Type::Reference(type_ref) => {
            rust_type_to_json_schema(&type_ref.elem, known_structs, known_enums)
        }
        _ => {
            panic!("build.rs: 无法解析的类型 {:?} in tool type struct.", ty);
        }
    }
}

/// 提取字段/struct 的 `///` 文档注释（syn 解析为 `#[doc = "..."]`），多行拼成一句。
fn extract_doc(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                lines.push(s.value().trim().to_string());
            }
        }
    }
    let joined = lines.join(" ").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// 将字符串转义为可安全嵌入 JSON 字面量的形式（描述文本用）。
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

fn generate_json_schema(
    item: &syn::ItemStruct,
    known_structs: &HashMap<String, syn::ItemStruct>,
    known_enums: &HashMap<String, Vec<String>>,
) -> String {
    let mut properties = Vec::new();
    let mut required = Vec::new();

    for field in &item.fields {
        let field_name = match &field.ident {
            Some(name) => name.to_string(),
            None => continue,
        };

        let is_option = is_option_type(&field.ty);
        let mut schema = rust_type_to_json_schema(&field.ty, known_structs, known_enums);
        if let Some(desc) = extract_doc(&field.attrs) {
            schema = schema.replacen(
                '{',
                &format!("{{ \"description\": \"{}\",", json_escape(&desc)),
                1,
            );
        }

        properties.push(format!("            \"{field_name}\": {schema}"));
        if !is_option {
            required.push(format!("            \"{field_name}\""));
        }
    }

    let properties_str = properties.join(",\n");
    let required_str = required.join(",\n");
    let sp = "    ";

    if required.is_empty() {
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
    }
}

/// 从 struct 定义提取字段，生成 ToolSchema impl。
fn generate_tool_schema_impl(
    item: &syn::ItemStruct,
    known_structs: &HashMap<String, syn::ItemStruct>,
    known_enums: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let struct_name = item.ident.to_string();
    let schema = generate_json_schema(item, known_structs, known_enums);
    let sp = "    ";

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
    // --- 1. ToolSchema 代码生成（syn 解析） ---
    // 当 AEMEATH_VERSION 环境变量变化时重新运行 build.rs
    // （AEMEATH_VERSION 由 release workflow 显式注入，本地 dev build 通常未设置）
    println!("cargo:rerun-if-env-changed=AEMEATH_VERSION");

    let out_dir = env::var("OUT_DIR").unwrap();
    let types_dir = Path::new("src/domain/types");

    if !types_dir.exists() {
        return;
    }

    println!("cargo:rerun-if-changed=src/domain/types");

    let mut impls = Vec::new();
    let skip_files = ["mod.rs"];

    // 第一遍：收集所有 struct 和 enum 定义
    let mut known_structs: HashMap<String, syn::ItemStruct> = HashMap::new();
    let mut known_enums: HashMap<String, Vec<String>> = HashMap::new();
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
            match item {
                syn::Item::Struct(item_struct) => {
                    if matches!(item_struct.vis, syn::Visibility::Public(_)) {
                        known_structs.insert(item_struct.ident.to_string(), item_struct.clone());
                    }
                }
                syn::Item::Enum(item_enum) => {
                    if matches!(item_enum.vis, syn::Visibility::Public(_)) {
                        let enum_name = item_enum.ident.to_string();
                        let variants: Vec<String> = item_enum
                            .variants
                            .iter()
                            .map(|v| {
                                // 将 PascalCase 变体名转换为 snake_case
                                let name = v.ident.to_string();
                                let mut snake_case = String::new();
                                for (i, c) in name.chars().enumerate() {
                                    if c.is_uppercase() {
                                        if i > 0 {
                                            snake_case.push('_');
                                        }
                                        snake_case.extend(c.to_lowercase());
                                    } else {
                                        snake_case.push(c);
                                    }
                                }
                                snake_case
                            })
                            .collect();
                        known_enums.insert(enum_name, variants);
                    }
                }
                _ => {}
            }
        }

        all_files.push((path.clone(), syn_file));
    }

    // Task-owned Tool output views participate in schema generation without
    // duplicating their definitions in Tools or Shared Kernel. Only the stable
    // TaskView and its wire enums may enter the Tools schema graph; aggregate
    // and command types stay private to Task.
    let task_types_path = Path::new("../task/src/domain/model.rs");
    println!("cargo:rerun-if-changed={}", task_types_path.display());
    let content = fs::read_to_string(task_types_path).unwrap();
    let syn_file: syn::File = syn::parse_str(&content)
        .unwrap_or_else(|e| panic!("build.rs: syn 解析 {} 失败: {e}", task_types_path.display()));
    for item in &syn_file.items {
        match item {
            syn::Item::Struct(item_struct) if item_struct.ident == "TaskView" => {
                known_structs.insert(item_struct.ident.to_string(), item_struct.clone());
            }
            syn::Item::Enum(item_enum)
                if matches!(
                    item_enum.ident.to_string().as_str(),
                    "TaskStatus" | "TaskPriority"
                ) =>
            {
                let variants = item_enum
                    .variants
                    .iter()
                    .map(|variant| {
                        let name = variant.ident.to_string();
                        let mut snake_case = String::new();
                        for (i, c) in name.chars().enumerate() {
                            if c.is_uppercase() {
                                if i > 0 {
                                    snake_case.push('_');
                                }
                                snake_case.extend(c.to_lowercase());
                            } else {
                                snake_case.push(c);
                            }
                        }
                        snake_case
                    })
                    .collect();
                known_enums.insert(item_enum.ident.to_string(), variants);
            }
            _ => {}
        }
    }
    all_files.push((task_types_path.to_path_buf(), syn_file));

    // 第二遍：为 Result struct 生成 ToolSchema impl
    for (path, syn_file) in &all_files {
        if path == task_types_path {
            continue;
        }
        let filename = path.file_name().unwrap().to_str().unwrap();
        if filename == "support.rs" {
            continue;
        }

        for item in &syn_file.items {
            if let syn::Item::Struct(item_struct) = item {
                if !matches!(item_struct.vis, syn::Visibility::Public(_)) {
                    continue;
                }
                if let Some(impl_code) =
                    generate_tool_schema_impl(item_struct, &known_structs, &known_enums)
                {
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
