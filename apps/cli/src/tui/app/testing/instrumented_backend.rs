use std::time::Instant;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

pub(crate) struct InstrumentedBackend<B> {
    inner: B,
}

impl<B> InstrumentedBackend<B> {
    pub(crate) const fn new(inner: B) -> Self {
        Self { inner }
    }

    pub(crate) const fn inner(&self) -> &B {
        &self.inner
    }

    pub(crate) fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Backend> Backend for InstrumentedBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let started = Instant::now();
        let content = content.collect::<Vec<_>>();
        let cells = content.len();
        let result = self.inner.draw(content.into_iter());
        crate::tui::render::performance::record_terminal_diff(cells, started.elapsed());
        result
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let started = Instant::now();
        let result = self.inner.flush();
        crate::tui::render::performance::record_backend_flush(started.elapsed());
        result
    }
}

#[cfg(test)]
#[path = "instrumented_backend_tests.rs"]
mod tests;
