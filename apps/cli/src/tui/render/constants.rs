//! 纯值常量（#1146 placement 归位）。
use once_cell::sync::Lazy;
use syntect::parsing::{SyntaxDefinition, SyntaxSet};

/// 全局主题集，使用 Catppuccin Macchiato，与 TUI palette 保持一致。
/// 全局主题集，使用 Catppuccin Macchiato，与 TUI palette 保持一致。
/// 全局语法集（懒加载，只加载一次）。
///
/// 在 syntect 默认语法集基础上合并内置的 TypeScript / TSX 语法（默认集不含 TS，
/// 资产由 microsoft/TypeScript-TmLanguage 转换而来，见 `assets/syntaxes/`）。
/// 全局语法集（懒加载，只加载一次）。
///
/// 在 syntect 默认语法集基础上合并内置的 TypeScript / TSX 语法（默认集不含 TS，
/// 资产由 microsoft/TypeScript-TmLanguage 转换而来，见 `assets/syntaxes/`）。
pub(crate) static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(|| {
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    for (asset_name, source) in [
        (
            "TypeScript.sublime-syntax",
            include_str!("../../../assets/syntaxes/TypeScript.sublime-syntax"),
        ),
        (
            "TSX.sublime-syntax",
            include_str!("../../../assets/syntaxes/TSX.sublime-syntax"),
        ),
    ] {
        let definition = SyntaxDefinition::load_from_str(source, true, None)
            .unwrap_or_else(|error| panic!("内置语法资产 {asset_name} 加载失败: {error}"));
        builder.add(definition);
    }
    builder.build()
});
