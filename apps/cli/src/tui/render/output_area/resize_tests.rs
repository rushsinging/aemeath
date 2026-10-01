use super::super::OutputArea;

fn output_area_with_term_width(term_width: usize) -> OutputArea {
    let mut output = OutputArea::new();
    output.term_width = term_width;
    output
}

#[test]
fn width_change_updates_term_width() {
    let mut output = output_area_with_term_width(78);

    output.handle_resize(100);

    assert_eq!(output.term_width, 98);
}

#[test]
fn same_width_keeps_term_width() {
    let mut output = output_area_with_term_width(78);

    output.handle_resize(80);

    assert_eq!(output.term_width, 78);
}
