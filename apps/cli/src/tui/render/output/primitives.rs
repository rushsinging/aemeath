mod convert;
pub mod diff;
pub mod fenced;
pub mod markdown;
pub mod table;
pub mod unified_diff;
pub mod wrap;

#[allow(unused_imports)]
pub use convert::{rendered_line_from_spanparts, spanparts_to_spans};
