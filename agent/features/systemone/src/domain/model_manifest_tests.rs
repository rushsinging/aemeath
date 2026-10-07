//! Model manifest 契约：schema 版本、生产维度、平台支持与资产安全规则的 fail-closed 校验。

use super::{required_platform, ModelManifest, ModelManifestError};
use serde_json::{json, Value};

/// 64 位小写十六进制的固定测试摘要（仅锁格式契约，不对应真实文件）。
const GGUF_SHA256: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const POINTER_HEAD_SHA256: &str =
    "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const TOKENIZER_SHA256: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

/// 满足全部契约的生产 manifest（hidden=1024、pointer=256、平台含 macos-aarch64）。
fn valid_manifest_value() -> Value {
    json!({
        "schema_version": 1,
        "engine_revision": "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b",
        "hidden_size": 1024,
        "pointer_dimension": 256,
        "temperature": 0.07,
        "supported_platforms": ["macos-aarch64"],
        "assets": [
            {
                "path": "model.gguf",
                "url": "https://models.example.com/systemone/model.gguf",
                "byte_length": 775000000u64,
                "sha256": GGUF_SHA256
            },
            {
                "path": "pointer_head.safetensors",
                "url": "https://models.example.com/systemone/pointer_head.safetensors",
                "byte_length": 8403456u64,
                "sha256": POINTER_HEAD_SHA256
            },
            {
                "path": "tokenizer/merges.txt",
                "url": "https://models.example.com/systemone/tokenizer/merges.txt",
                "byte_length": 263456u64,
                "sha256": TOKENIZER_SHA256
            }
        ]
    })
}

fn parse(value: &Value) -> Result<ModelManifest, ModelManifestError> {
    ModelManifest::parse(&value.to_string())
}

/// 覆写指定路径资产的字段。
fn set_asset_field(value: &mut Value, path: &str, field: &str, replacement: Value) {
    let assets = value["assets"]
        .as_array_mut()
        .expect("manifest 含 assets 数组");
    for asset in assets {
        if asset["path"].as_str() == Some(path) {
            asset[field] = replacement;
            return;
        }
    }
    panic!("未找到路径为 {path} 的资产");
}

#[test]
fn parse_when_manifest_is_valid_returns_contract_fields() {
    let manifest = parse(&valid_manifest_value()).expect("合法 manifest 应解析并校验通过");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.engine_revision,
        "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b"
    );
    assert_eq!(manifest.hidden_size, 1024);
    assert_eq!(manifest.pointer_dimension, 256);
    assert!((manifest.temperature - 0.07).abs() <= f32::EPSILON);
    assert_eq!(
        manifest.supported_platforms,
        vec!["macos-aarch64".to_owned()]
    );
    assert_eq!(manifest.assets.len(), 3);
    assert_eq!(manifest.assets[0].path, "model.gguf");
    assert_eq!(manifest.assets[0].byte_length, 775_000_000);
    assert_eq!(manifest.assets[0].sha256, GGUF_SHA256);
    assert_eq!(manifest.assets[2].path, "tokenizer/merges.txt");
    assert!(manifest.validate_for("macos-aarch64").is_ok());
}

#[test]
fn parse_when_required_field_missing_returns_schema_error() {
    let mut value = valid_manifest_value();
    value
        .as_object_mut()
        .expect("manifest 为 JSON 对象")
        .remove("hidden_size");
    let error = parse(&value).expect_err("缺失字段必须拒绝");
    assert!(
        matches!(&error, ModelManifestError::Schema(detail) if detail.contains("hidden_size")),
        "实际错误：{error:?}"
    );
}

#[test]
fn parse_when_unknown_manifest_field_present_returns_schema_error() {
    let mut value = valid_manifest_value();
    value["debug"] = json!(true);
    let error = parse(&value).expect_err("未知字段必须拒绝");
    assert!(
        matches!(&error, ModelManifestError::Schema(detail) if detail.contains("unknown field")),
        "实际错误：{error:?}"
    );
}

#[test]
fn parse_when_unknown_asset_field_present_returns_schema_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "md5", json!("abc"));
    let error = parse(&value).expect_err("资产条目未知字段必须拒绝");
    assert!(
        matches!(&error, ModelManifestError::Schema(detail) if detail.contains("unknown field")),
        "实际错误：{error:?}"
    );
}

#[test]
fn parse_when_schema_version_is_not_current_returns_error() {
    let mut value = valid_manifest_value();
    value["schema_version"] = json!(2);
    let error = parse(&value).expect_err("非当前 schema 版本必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsupportedSchemaVersion {
            found: 2,
            expected: 1
        }
    );
}

#[test]
fn parse_when_schema_version_two_with_new_field_returns_unsupported_version() {
    let mut value = valid_manifest_value();
    value["schema_version"] = json!(2);
    value["future_field"] = json!("added-in-v2");
    let error = parse(&value).expect_err("v2 + 新字段必须按版本不支持拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsupportedSchemaVersion {
            found: 2,
            expected: 1
        }
    );
}

#[test]
fn parse_when_schema_version_missing_returns_schema_error() {
    let mut value = valid_manifest_value();
    value
        .as_object_mut()
        .expect("manifest 为 JSON 对象")
        .remove("schema_version");
    let error = parse(&value).expect_err("缺 schema_version 必须拒绝");
    assert!(
        matches!(&error, ModelManifestError::Schema(detail) if detail.contains("schema_version")),
        "实际错误：{error:?}"
    );
}

#[test]
fn validate_locks_production_dimensions_to_1024_and_256() {
    let mut manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");

    manifest.hidden_size = 1023;
    assert_eq!(
        manifest.validate(),
        Err(ModelManifestError::UnsupportedHiddenSize {
            found: 1023,
            expected: 1024
        })
    );

    manifest.hidden_size = 1024;
    manifest.pointer_dimension = 255;
    assert_eq!(
        manifest.validate(),
        Err(ModelManifestError::UnsupportedPointerDimension {
            found: 255,
            expected: 256
        })
    );

    manifest.pointer_dimension = 256;
    assert!(manifest.validate().is_ok());
}

#[test]
fn validate_when_temperature_is_not_finite_positive_returns_error() {
    for temperature in [0.0, -0.5] {
        let mut value = valid_manifest_value();
        value["temperature"] = json!(temperature);
        let error = parse(&value).expect_err("非正 temperature 必须拒绝");
        assert_eq!(error, ModelManifestError::InvalidTemperature);
    }

    let mut manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    manifest.temperature = f32::NAN;
    assert_eq!(
        manifest.validate(),
        Err(ModelManifestError::InvalidTemperature)
    );
    manifest.temperature = f32::INFINITY;
    assert_eq!(
        manifest.validate(),
        Err(ModelManifestError::InvalidTemperature)
    );
}

#[test]
fn validate_when_engine_revision_is_blank_returns_error() {
    let mut value = valid_manifest_value();
    value["engine_revision"] = json!("   ");
    let error = parse(&value).expect_err("空白 engine_revision 必须拒绝");
    assert_eq!(error, ModelManifestError::EmptyEngineRevision);
}

#[test]
fn validate_when_engine_revision_escapes_parent_directory_returns_error() {
    for revision in ["../x", "a/../b", "abc..def"] {
        let mut value = valid_manifest_value();
        value["engine_revision"] = json!(revision);
        let error = parse(&value).expect_err("含 .. 的 engine_revision 必须拒绝");
        assert_eq!(
            error,
            ModelManifestError::InvalidEngineRevision {
                revision: revision.to_owned()
            }
        );
    }
}

#[test]
fn validate_when_engine_revision_starts_with_dot_returns_error() {
    let mut value = valid_manifest_value();
    value["engine_revision"] = json!(".hidden");
    let error = parse(&value).expect_err("点开头的 engine_revision 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidEngineRevision {
            revision: ".hidden".to_owned()
        }
    );
}

#[test]
fn validate_when_engine_revision_exceeds_max_length_returns_error() {
    let revision = "a".repeat(65);
    let mut value = valid_manifest_value();
    value["engine_revision"] = json!(revision);
    let error = parse(&value).expect_err("超长 engine_revision 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidEngineRevision { revision }
    );
}

#[test]
fn validate_when_engine_revision_contains_separator_returns_error() {
    for revision in ["abc/def", "abc\\def"] {
        let mut value = valid_manifest_value();
        value["engine_revision"] = json!(revision);
        let error = parse(&value).expect_err("含路径分隔符的 engine_revision 必须拒绝");
        assert_eq!(
            error,
            ModelManifestError::InvalidEngineRevision {
                revision: revision.to_owned()
            }
        );
    }
}

#[test]
fn validate_when_engine_revision_contains_space_returns_error() {
    let mut value = valid_manifest_value();
    value["engine_revision"] = json!("rev 1");
    let error = parse(&value).expect_err("含空白的 engine_revision 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidEngineRevision {
            revision: "rev 1".to_owned()
        }
    );
}

#[test]
fn validate_when_engine_revision_is_canonical_segment_returns_ok() {
    let max_length_revision = "a".repeat(64);
    for revision in [
        "0",
        "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b",
        "v1.2.3_rc-1",
        max_length_revision.as_str(),
    ] {
        let mut value = valid_manifest_value();
        value["engine_revision"] = json!(revision);
        parse(&value).expect("canonical segment 形态的 engine_revision 应通过");
    }
}

#[test]
fn validate_when_supported_platforms_are_empty_returns_error() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!([]);
    let error = parse(&value).expect_err("空平台列表必须拒绝");
    assert_eq!(error, ModelManifestError::EmptySupportedPlatforms);
}

#[test]
fn validate_when_supported_platforms_blank_label_returns_error() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!(["macos-aarch64", " "]);
    let error = parse(&value).expect_err("空白平台标识必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidPlatformLabel {
            platform: " ".to_owned()
        }
    );
}

#[test]
fn validate_when_supported_platforms_inner_whitespace_returns_error() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!(["macos aarch64", "macos-aarch64"]);
    let error = parse(&value).expect_err("平台标识内部空白必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidPlatformLabel {
            platform: "macos aarch64".to_owned()
        }
    );
}

#[test]
fn validate_when_supported_platforms_control_character_returns_error() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!(["macos\u{1}aarch64", "macos-aarch64"]);
    let error = parse(&value).expect_err("平台标识控制字符必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidPlatformLabel {
            platform: "macos\u{1}aarch64".to_owned()
        }
    );
}

#[test]
fn validate_when_required_platform_absent_returns_error() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!(["linux-x86_64"]);
    let error = parse(&value).expect_err("缺少 macos-aarch64 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::RequiredPlatformMissing {
            platform: "macos-aarch64".to_owned()
        }
    );
}

#[test]
fn validate_for_when_target_platform_is_supported_returns_ok() {
    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!(["macos-aarch64", "linux-x86_64"]);
    let manifest = parse(&value).expect("合法 manifest 应通过");
    assert!(manifest.validate_for("macos-aarch64").is_ok());
    assert!(manifest.validate_for("linux-x86_64").is_ok());
}

#[test]
fn validate_for_when_target_platform_absent_returns_error() {
    let manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    let error = manifest
        .validate_for("windows-x86_64")
        .expect_err("未列入支持列表的平台必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsupportedPlatform {
            platform: "windows-x86_64".to_owned()
        }
    );
}

#[test]
fn validate_for_when_target_platform_contains_whitespace_returns_error() {
    let manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    let error = manifest
        .validate_for("macos-aarch64 ")
        .expect_err("带空白的目标平台必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsupportedPlatform {
            platform: "macos-aarch64 ".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_is_absolute_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "path", json!("/srv/model.gguf"));
    let error = parse(&value).expect_err("绝对路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "/srv/model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_escapes_parent_directory_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/../../secrets.txt"),
    );
    let error = parse(&value).expect_err("包含 .. 的路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer/../../secrets.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_is_empty_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "path", json!(""));
    let error = parse(&value).expect_err("空路径必须拒绝");
    assert_eq!(error, ModelManifestError::EmptyAssetPath);
}

#[test]
fn validate_when_asset_path_is_directory_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/"),
    );
    let error = parse(&value).expect_err("目录形态路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::DirectoryAssetPath {
            path: "tokenizer/".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_contains_backslash_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer\\merges.txt"),
    );
    let error = parse(&value).expect_err("反斜杠路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer\\merges.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_contains_whitespace_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/m erges.txt"),
    );
    let error = parse(&value).expect_err("含空白的路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer/m erges.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_contains_control_character_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/merge\u{7}s.txt"),
    );
    let error = parse(&value).expect_err("含控制字符的路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer/merge\u{7}s.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_contains_non_ascii_character_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/合并.txt"),
    );
    let error = parse(&value).expect_err("非 ASCII 路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer/合并.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_path_contains_disallowed_ascii_character_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/merges@.txt"),
    );
    let error = parse(&value).expect_err("非 [-_.] 的路径字符必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsafeAssetPath {
            path: "tokenizer/merges@.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_paths_differ_only_by_case_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.push(json!({
        "path": "Model.GGUF",
        "url": "https://models.example.com/systemone/Model.GGUF",
        "byte_length": 100u64,
        "sha256": GGUF_SHA256
    }));
    let error = parse(&value).expect_err("大小写物理冲突的路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::DuplicateAssetPath {
            path: "Model.GGUF".to_owned()
        }
    );
}

#[test]
fn validate_when_model_gguf_asset_missing_but_nested_present_returns_count_zero() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.remove(0);
    assets.push(json!({
        "path": "nested/model.gguf",
        "url": "https://models.example.com/systemone/nested/model.gguf",
        "byte_length": 100u64,
        "sha256": GGUF_SHA256
    }));
    let error = parse(&value).expect_err("nested model.gguf 不算 model 角色");
    assert_eq!(error, ModelManifestError::ModelGgufCount { found: 0 });
}

#[test]
fn validate_when_pointer_head_asset_missing_but_nested_present_returns_count_zero() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.remove(1);
    assets.push(json!({
        "path": "nested/pointer_head.safetensors",
        "url": "https://models.example.com/systemone/nested/pointer_head.safetensors",
        "byte_length": 100u64,
        "sha256": POINTER_HEAD_SHA256
    }));
    let error = parse(&value).expect_err("nested pointer_head.safetensors 不算 pointer 角色");
    assert_eq!(
        error,
        ModelManifestError::PointerHeadAssetCount { found: 0 }
    );
}

#[test]
fn validate_when_tokenizer_file_name_uses_allowed_ascii_characters_returns_ok() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/merges-v1_2.final.txt"),
    );
    parse(&value).expect("[-_.] 与字母数字构成的 tokenizer 文件名应通过");
}

#[test]
fn validate_when_tokenizer_asset_in_subdirectory_returns_ok() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer/vocab/merges.txt"),
    );
    parse(&value).expect("tokenizer 子目录下的文件应通过");
}

#[test]
fn validate_when_asset_path_duplicated_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    let duplicate = json!({
        "path": "model.gguf",
        "url": "https://models.example.com/systemone/model.gguf",
        "byte_length": 775000000u64,
        "sha256": GGUF_SHA256
    });
    assets.push(duplicate);
    let error = parse(&value).expect_err("重复路径必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::DuplicateAssetPath {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_is_empty_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "url", json!(""));
    let error = parse(&value).expect_err("空 URL 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_lacks_https_scheme_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("models.example.com/model.gguf"),
    );
    let error = parse(&value).expect_err("缺少 http(s) scheme 的 URL 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_uses_http_scheme_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("http://models.example.com/model.gguf"),
    );
    let error = parse(&value).expect_err("http 下载地址必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_host_is_query_only_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("https://?path=model.gguf"),
    );
    let error = parse(&value).expect_err("query-only host 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_host_is_fragment_only_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("https://#model.gguf"),
    );
    let error = parse(&value).expect_err("fragment-only host 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_contains_whitespace_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("https://models.example.com/mo del.gguf"),
    );
    let error = parse(&value).expect_err("含空白的 URL 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetUrl {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_url_is_https_with_query_returns_ok() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "model.gguf",
        "url",
        json!("https://models.example.com/model.gguf?token=abc"),
    );
    parse(&value).expect("带 query 的 https 地址应通过");
}

#[test]
fn validate_when_sha256_is_uppercase_returns_error() {
    let mut value = valid_manifest_value();
    let uppercase = GGUF_SHA256.to_uppercase();
    set_asset_field(&mut value, "model.gguf", "sha256", json!(uppercase));
    let error = parse(&value).expect_err("大写 SHA 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetSha256 {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_sha256_length_is_wrong_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "sha256", json!("0f1e2d3c"));
    let error = parse(&value).expect_err("非 64 位的 SHA 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetSha256 {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_sha256_contains_non_hex_character_returns_error() {
    let mut value = valid_manifest_value();
    let non_hex = format!("g{}", &GGUF_SHA256[1..]);
    set_asset_field(&mut value, "model.gguf", "sha256", json!(non_hex));
    let error = parse(&value).expect_err("非十六进制 SHA 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::InvalidAssetSha256 {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_asset_byte_length_is_zero_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(&mut value, "model.gguf", "byte_length", json!(0));
    let error = parse(&value).expect_err("零字节资产必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::ZeroAssetByteLength {
            path: "model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_model_gguf_asset_missing_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.remove(0);
    let error = parse(&value).expect_err("缺少 model.gguf 必须拒绝");
    assert_eq!(error, ModelManifestError::ModelGgufCount { found: 0 });
}

#[test]
fn validate_when_model_gguf_asset_duplicated_in_nested_directory_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.push(json!({
        "path": "nested/model.gguf",
        "url": "https://models.example.com/systemone/nested/model.gguf",
        "byte_length": 100u64,
        "sha256": GGUF_SHA256
    }));
    let error = parse(&value).expect_err("生产布局外的 nested model.gguf 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnexpectedAssetPath {
            path: "nested/model.gguf".to_owned()
        }
    );
}

#[test]
fn validate_when_pointer_head_asset_missing_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.remove(1);
    let error = parse(&value).expect_err("缺少 pointer_head.safetensors 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::PointerHeadAssetCount { found: 0 }
    );
}

#[test]
fn validate_when_pointer_head_asset_duplicated_in_nested_directory_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.push(json!({
        "path": "nested/pointer_head.safetensors",
        "url": "https://models.example.com/systemone/nested/pointer_head.safetensors",
        "byte_length": 100u64,
        "sha256": POINTER_HEAD_SHA256
    }));
    let error = parse(&value).expect_err("生产布局外的 nested pointer_head 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnexpectedAssetPath {
            path: "nested/pointer_head.safetensors".to_owned()
        }
    );
}

#[test]
fn validate_when_extra_asset_outside_production_layout_returns_error() {
    let mut value = valid_manifest_value();
    let assets = value["assets"].as_array_mut().expect("assets 数组");
    assets.push(json!({
        "path": "extra.txt",
        "url": "https://models.example.com/systemone/extra.txt",
        "byte_length": 10u64,
        "sha256": TOKENIZER_SHA256
    }));
    let error = parse(&value).expect_err("生产布局外的资产必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnexpectedAssetPath {
            path: "extra.txt".to_owned()
        }
    );
}

#[test]
fn validate_when_tokenizer_asset_missing_returns_error() {
    let mut value = valid_manifest_value();
    set_asset_field(
        &mut value,
        "tokenizer/merges.txt",
        "path",
        json!("tokenizer"),
    );
    let error = parse(&value).expect_err("缺少 tokenizer/ 下文件必须拒绝");
    assert_eq!(error, ModelManifestError::MissingTokenizerAsset);
}

#[test]
fn asset_accessors_return_role_structures_after_validation() {
    let manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    let model = manifest.model_asset().expect("model 角色应存在");
    assert_eq!(model.path, "model.gguf");
    assert_eq!(model.byte_length, 775_000_000);
    let pointer_head = manifest.pointer_head_asset().expect("pointer 角色应存在");
    assert_eq!(pointer_head.path, "pointer_head.safetensors");
    let tokenizer = manifest.tokenizer_assets().expect("tokenizer 资产应存在");
    assert_eq!(tokenizer.len(), 1);
    assert_eq!(tokenizer[0].path, "tokenizer/merges.txt");
}

#[test]
fn model_asset_accessor_when_role_missing_returns_count_error() {
    let mut manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    manifest.assets.remove(0);
    assert_eq!(
        manifest.model_asset(),
        Err(ModelManifestError::ModelGgufCount { found: 0 })
    );
}

#[test]
fn pointer_head_asset_accessor_when_role_missing_returns_count_error() {
    let mut manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    manifest.assets.remove(1);
    assert_eq!(
        manifest.pointer_head_asset(),
        Err(ModelManifestError::PointerHeadAssetCount { found: 0 })
    );
}

#[test]
fn tokenizer_assets_accessor_when_empty_returns_missing_error() {
    let mut manifest = parse(&valid_manifest_value()).expect("合法 manifest 应通过");
    manifest.assets.pop();
    assert_eq!(
        manifest.tokenizer_assets(),
        Err(ModelManifestError::MissingTokenizerAsset)
    );
}

#[test]
fn required_platform_is_single_source_for_manifest_validation() {
    assert_eq!(required_platform(), "macos-aarch64");

    let mut value = valid_manifest_value();
    value["supported_platforms"] = json!([required_platform()]);
    parse(&value).expect("支持列表含 required_platform() 应通过");

    value["supported_platforms"] = json!(["linux-x86_64"]);
    let error = parse(&value).expect_err("缺少 required_platform() 必须拒绝");
    assert_eq!(
        error,
        ModelManifestError::RequiredPlatformMissing {
            platform: required_platform().to_owned()
        }
    );
}

#[test]
fn error_display_messages_contain_chinese_text() {
    let mut size_value = valid_manifest_value();
    size_value["hidden_size"] = json!(512);
    let size_error = parse(&size_value).expect_err("非 1024 必须拒绝");

    let mut url_value = valid_manifest_value();
    set_asset_field(&mut url_value, "model.gguf", "byte_length", json!(0));
    let length_error = parse(&url_value).expect_err("零字节资产必须拒绝");

    for error in [size_error, length_error] {
        assert!(
            error
                .to_string()
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)),
            "错误消息应包含中文：{error}"
        );
    }
}
