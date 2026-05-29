mod convert;
pub mod diff;
pub mod markdown;
pub mod table;

#[allow(unused_imports)]
pub use convert::{rendered_line_from_spanparts, spanparts_to_spans};
