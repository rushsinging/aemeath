use super::InputArea;

impl InputArea {
    pub fn input_content_width(area_width: u16) -> u16 {
        area_width.saturating_sub(2)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn input_content_width_accounts_for_border() {
        assert_eq!(InputArea::input_content_width(80), 78);
    }

    #[test]
    fn input_content_width_saturates_small_width() {
        assert_eq!(InputArea::input_content_width(1), 0);
    }
}
