//! An app that is nothing but its record closure.

use crate::app::App;
use crate::ui::Ui;
use crate::window::window_token::WindowToken;

/// An [`App`] that is nothing but its record closure — what every test
/// driving frames actually wants.
#[derive(Debug)]
pub struct RecordApp<F> {
    record: F,
}

impl<F: FnMut(&mut Ui)> RecordApp<F> {
    /// Wrap a record closure as an [`App`].
    pub fn new(record: F) -> Self {
        Self { record }
    }
}

impl<F: FnMut(&mut Ui)> App for RecordApp<F> {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        (self.record)(ui);
    }
}
