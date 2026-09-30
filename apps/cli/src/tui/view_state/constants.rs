//! 纯值常量（#1146 placement 归位）。

pub(crate) const MAX_RENDER_LINES: usize = 3_000;

pub(crate) const MIN_HISTORY_LOAD_BATCH_LINES: usize = 15;

pub(crate) const HISTORY_LOAD_PERCENT: usize = 60;

pub(crate) const INITIAL_RENDER_LINES: usize = 1_000;

pub(crate) const DEFAULT_VERB: &str = "Thinking";

/// 装饰性动词池。verb 选定移入 view_state 后，此处为该池的唯一真相来源
/// （TaskData 4.1 已删除原 `render/output_area/spinner.rs::SPINNER_VERBS`）。
/// 装饰性动词池。verb 选定移入 view_state 后，此处为该池的唯一真相来源
/// （TaskData 4.1 已删除原 `render/output_area/spinner.rs::SPINNER_VERBS`）。
pub(crate) const SPINNER_VERBS: &[&str] = &[
    "Thinking",
    "Pondering",
    "Crafting",
    "Computing",
    "Brewing",
    "Weaving",
    "Conjuring",
    "Forging",
    "Hatching",
    "Cooking",
    "Channeling",
    "Ruminating",
    "Composing",
    "Imagining",
    "Processing",
    "Puzzling",
    "Mulling",
    "Noodling",
    "Tinkering",
    "Crystallizing",
    "Synthesizing",
    "Architecting",
    "Orchestrating",
    "Incubating",
    "Fermenting",
    "Simmering",
    "Percolating",
    "Cogitating",
    "Meandering",
    "Harmonizing",
];
