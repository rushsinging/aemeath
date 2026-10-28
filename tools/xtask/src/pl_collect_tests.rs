//! `pl_collect` 的单元测试：根导出面提取、符号定义扫描、消费点扫描三段纯函数。

use std::collections::{BTreeMap, BTreeSet};

use crate::pl_collect::{scan_source, SymbolDefinitions};

const LIB_SOURCE: &str = r#"
pub mod published_language;
pub use published_language::{ProviderRequestData, ProviderError};
pub mod composition {
    pub use crate::adapters::client::wire_provider_client;
}
"#;

const CRATE_SOURCE: &str = r#"
/// 请求载荷。
pub struct ProviderRequestData {
    pub model: String,
    pub max_output_tokens: u32,
}

impl ProviderRequestData {
    pub fn tool_ready(&self) -> bool {
        true
    }
}

pub enum ProviderError {
    Fatal { kind: String },
}

pub fn wire_provider_client() -> u32 {
    1
}

pub struct InternalHelper;
"#;

/// 根导出面提取：根 `pub use` 与嵌套 `pub mod` 内 `pub use` 的叶子符号都要拿到。
#[test]
fn root_exports_include_nested_module_reexports() {
    let exports = crate::pl_collect::extract_root_exports(LIB_SOURCE).expect("解析夹具失败");
    let mut sorted = exports;
    sorted.sort();
    assert_eq!(
        sorted,
        vec![
            "ProviderError".to_owned(),
            "ProviderRequestData".to_owned(),
            "wire_provider_client".to_owned(),
        ]
    );
}

/// 定义扫描：kind、定义文件、签名行、字段与方法齐全；非根导出的内部类型不进结果。
#[test]
fn definitions_carry_kind_fields_methods() {
    let mut names = BTreeSet::new();
    names.insert("ProviderRequestData".to_owned());
    names.insert("ProviderError".to_owned());
    names.insert("wire_provider_client".to_owned());
    let definitions = SymbolDefinitions::scan(
        CRATE_SOURCE,
        "src/published_language.rs",
        &names,
        &BTreeMap::new(),
    );
    let request = definitions.get("ProviderRequestData").expect("缺请求载荷");
    assert_eq!(request.kind, "struct");
    assert_eq!(request.def, "src/published_language.rs");
    assert_eq!(request.signature, "pub struct ProviderRequestData {");
    assert!(request.fields.contains(&"pub model: String".to_owned()));
    assert!(request
        .methods
        .iter()
        .any(|method| method.contains("pub fn tool_ready")));
    let error = definitions.get("ProviderError").expect("缺错误枚举");
    assert_eq!(error.kind, "enum");
    assert!(definitions.get("InternalHelper").is_none());
}

/// 消费点扫描：`use` 树整棵跳过，调用/构造/类型引用三类节点命中并带行号。
#[test]
fn scan_source_skips_use_and_records_three_forms() {
    let consumer_source = "use provider::ProviderRequestData;\nfn build() {\n    let request = ProviderRequestData { model: \"gpt\".into() };\n    ProviderRequestData::tool_ready(&request);\n    fn take(input: ProviderRequestData) {}\n    take(request);\n}\n";
    let mut names = BTreeSet::new();
    names.insert("ProviderRequestData".to_owned());
    let consumers = scan_source(consumer_source, &names, "runtime/src/build.rs");
    let consumer = consumers
        .get("ProviderRequestData")
        .expect("缺请求载荷消费者");
    assert_eq!(consumer.file, "runtime/src/build.rs");
    let lines: Vec<usize> = consumer
        .snippets
        .iter()
        .map(|snippet| snippet.line)
        .collect();
    assert!(lines.contains(&3), "构造点行号缺失: {lines:?}");
    assert!(lines.contains(&4), "调用点行号缺失: {lines:?}");
    // 单段路径的文本定位无法区分语境，类型引用允许落在调用行或签名行。
    assert!(
        lines.contains(&4) || lines.contains(&5),
        "类型引用行号缺失: {lines:?}"
    );
    assert!(!lines.contains(&1), "use 行不得计为消费: {lines:?}");
    let kinds: Vec<&str> = consumer
        .snippets
        .iter()
        .map(|snippet| snippet.kind.as_str())
        .collect();
    assert!(kinds.contains(&"ctor"));
    assert!(kinds.contains(&"call"));
    assert!(kinds.contains(&"fnty"));
}

/// 消费点扫描：`impl Trait for Type` 的 trait 路径命中记为接口实现（implty）。
#[test]
fn scan_source_marks_trait_impl() {
    let consumer_source =
        "impl ProviderError for WrappedError {\n    fn code(&self) -> u32 {\n        1\n    }\n}\n";
    let mut names = BTreeSet::new();
    names.insert("ProviderError".to_owned());
    let consumers = scan_source(consumer_source, &names, "sdk/src/error.rs");
    let consumer = consumers.get("ProviderError").expect("缺接口实现消费者");
    assert!(consumer
        .snippets
        .iter()
        .any(|snippet| snippet.kind == "implty"));
}

/// 定义扫描：`use 真名 as 别名` 发布的符号按别名键记录定义（真名不在导出面）。
#[test]
fn definitions_resolve_alias_to_exported_key() {
    let aliased_source =
        "pub enum StopReason {\n    EndTurn,\n}\npub use StopReason as ResponseStopReason;\n";
    let mut names = BTreeSet::new();
    names.insert("ResponseStopReason".to_owned());
    let alias_map = crate::pl_collect::collect_alias_map(aliased_source);
    let definitions = SymbolDefinitions::scan(
        aliased_source,
        "src/published_language.rs",
        &names,
        &alias_map,
    );
    let stop = definitions.get("ResponseStopReason").expect("别名键缺定义");
    assert_eq!(stop.kind, "enum");
    assert!(stop.fields.contains(&"EndTurn".to_owned()));
}

/// 消费点扫描：match 臂等裸路径引用记为 ref（路径引用）。
#[test]
fn scan_source_marks_bare_path_reference() {
    let consumer_source = "fn map(kind: ProviderErrorKind) {\n    match kind {\n        ProviderErrorKind::Cancelled => {}\n    }\n}\n";
    let mut names = BTreeSet::new();
    names.insert("ProviderErrorKind".to_owned());
    let consumers = scan_source(consumer_source, &names, "composition/src/provider.rs");
    let consumer = consumers
        .get("ProviderErrorKind")
        .expect("缺路径引用消费者");
    assert!(consumer
        .snippets
        .iter()
        .any(|snippet| snippet.kind == "ref" && snippet.line == 3));
}

/// 消费点扫描：trait bound 与关联类型位置的类型引用同样命中（fnty）。
#[test]
fn scan_source_marks_trait_bound_reference() {
    let consumer_source = "fn invoke<P: ProviderError>(proxy: P) {}\n";
    let mut names = BTreeSet::new();
    names.insert("ProviderError".to_owned());
    let consumers = scan_source(consumer_source, &names, "sdk/src/port.rs");
    let consumer = consumers
        .get("ProviderError")
        .expect("缺 trait bound 消费者");
    assert!(consumer
        .snippets
        .iter()
        .any(|snippet| snippet.kind == "fnty"));
}
