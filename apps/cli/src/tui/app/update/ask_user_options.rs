pub(super) fn build_option_line_ranges(
    start: usize,
    options: &[String],
) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::with_capacity(options.len());
    let mut next = start;
    for option in options {
        let line_count = option.lines().count().max(1);
        ranges.push(next..next + line_count);
        next += line_count;
    }
    ranges
}
