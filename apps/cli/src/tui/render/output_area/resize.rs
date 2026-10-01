impl super::OutputArea {
    pub fn handle_resize(&mut self, width: u16) {
        let new_term_width = (width as usize).saturating_sub(2);
        if new_term_width != self.term_width {
            self.term_width = new_term_width;
        }
    }
}

#[cfg(test)]
#[path = "resize_tests.rs"]
mod tests;
