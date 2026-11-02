//! The overlays recording now, innermost last, and the close a row in each asked for.

/// One entry per overlay body recording now, innermost last: whether a row in it asked to close
/// it. A [`MenuItem`](crate::MenuItem) closes the overlay it sits in through it, so a row takes
/// no handle. Capacity retained, so no frame allocates after the deepest nesting.
#[derive(Debug, Default)]
pub(crate) struct OverlayStack {
    closes: Vec<bool>,
}

impl OverlayStack {
    /// An overlay body starts recording.
    pub(crate) fn enter(&mut self) {
        self.closes.push(false);
    }

    /// The innermost overlay body ends; whether a row in it asked to close it.
    pub(crate) fn leave(&mut self) -> bool {
        self.closes
            .pop()
            .expect("an overlay leaves only after it entered")
    }

    /// Ask the innermost overlay recording now to close. No-op outside every overlay, as a row
    /// recorded in the main tree has nothing to close.
    pub(crate) fn close_innermost(&mut self) {
        if let Some(close) = self.closes.last_mut() {
            *close = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widget_core::overlay_stack::OverlayStack;

    /// A close reaches the innermost overlay alone; one asked outside every overlay is dropped.
    #[test]
    fn a_close_reaches_the_innermost_overlay() {
        let mut stack = OverlayStack::default();
        stack.close_innermost();
        stack.enter();
        stack.enter();
        stack.close_innermost();
        assert!(stack.leave(), "the inner overlay closes");
        assert!(!stack.leave(), "the outer one stays open");
    }
}
