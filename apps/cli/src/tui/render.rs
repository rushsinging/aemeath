// diff 原语（output/diff.rs + primitives/diff）已接入 Edit 工具结果渲染（refs #58/#61），
// 不再依赖此 allow。仍保留是为其余未接线的渲染原语（safe_text/display/syntax/theme 等
// 其他 gap 项）兜底；待对应 gap 接线后逐项移除。
#![allow(dead_code)]

pub mod dialog;
pub mod display;
pub mod input;
pub mod output;
pub mod output_area;
pub mod status;
pub mod syntax;
pub mod theme;
