use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Type};

#[proc_macro_derive(ToolSchema)]
pub fn derive_tool_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("ToolSchema only supports structs with named fields"),
        },
        _ => panic!("ToolSchema only supports structs"),
    };

    let field_entries: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .map(|f| {
            let field_name = f.ident.as_ref().unwrap().to_string();
            let field_type = &f.ty;
            let (json_type, optional) = type_to_json_schema(field_type);

            if optional {
                quote! {
                    (#field_name.to_string(), serde_json::json!({
                        "type": #json_type,
                        "optional": true
                    }))
                }
            } else {
                quote! {
                    (#field_name.to_string(), serde_json::json!({
                        "type": #json_type
                    }))
                }
            }
        })
        .collect();

    let expanded = quote! {
        impl crate::tool::types::ToolSchema for #name {
            fn data_schema() -> serde_json::Value {
                let fields: Vec<(String, serde_json::Value)> = vec![
                    #(#field_entries),*
                ];
                serde_json::json!({
                    "type": "object",
                    "properties": serde_json::Value::Object(
                        fields.into_iter()
                            .map(|(k, v)| (k, v))
                            .collect::<serde_json::Map<String, serde_json::Value>>()
                    )
                })
            }
        }
    };

    TokenStream::from(expanded)
}

fn type_to_json_schema(ty: &Type) -> (&'static str, bool) {
    match ty {
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last().unwrap();
            let type_name = segment.ident.to_string();
            match type_name.as_str() {
                "String" | "PathBuf" | "str" => ("string", false),
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize"
                | "usize" => ("integer", false),
                "f32" | "f64" => ("number", false),
                "bool" => ("boolean", false),
                "Vec" => ("array", false),
                "Option" => {
                    if let Some(inner) = extract_option_inner(type_path) {
                        let (inner_type, _) = type_to_json_schema(inner);
                        (inner_type, true)
                    } else {
                        ("object", true)
                    }
                }
                _ => ("object", false),
            }
        }
        _ => ("object", false),
    }
}

fn extract_option_inner(type_path: &syn::TypePath) -> Option<&Type> {
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            args.args.first().and_then(|arg| match arg {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    // proc-macro crate 不能直接测试，需要集成测试
}
