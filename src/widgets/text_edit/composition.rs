//! The input method's live composition, as a field shows it.

use crate::input::ime_preedit::ImePreedit;
use std::ops::Range;

/// What the input method is composing, copied out of `Ui` so the pass can show it while
/// holding `&mut Ui`, and the buffer shown with it.
#[derive(Clone, Default, Debug)]
pub(super) struct Composition {
    /// The uncommitted text; stale when not composing.
    preedit: String,
    /// The buffer with the preedit spliced in at the caret: the run a composing field shapes.
    display: String,
    /// A composition was live as of the last pass.
    active: bool,
}

/// What [`Composition::roll`] found this pass.
#[derive(Clone, Copy, Debug)]
pub(super) struct CompositionPass {
    /// A composition starts this pass. It types nothing until it commits, but deletes the
    /// selection first, as browsers do.
    pub(super) started: bool,
    /// The input method's cursor in the preedit, as a byte offset; `None` when not
    /// composing.
    pub(super) cursor: Option<usize>,
}

/// The run a field shows, the caret in it, and the range it washes.
#[derive(Clone, Debug)]
pub(super) struct Shown<'a> {
    pub(super) text: &'a str,
    pub(super) caret: usize,
    pub(super) wash: Option<Range<usize>>,
}

impl Composition {
    /// Roll onto this pass's `preedit`, `None` when the field is not composing.
    pub(super) fn roll(&mut self, preedit: Option<ImePreedit<'_>>) -> CompositionPass {
        let cursor = preedit.map(|preedit| {
            self.preedit.clear();
            self.preedit.push_str(preedit.text);
            preedit
                .cursor
                .map_or(preedit.text.len(), |cursor| cursor.range().end)
        });
        let composing = cursor.is_some();
        let started = composing && !self.active;
        self.active = composing;
        CompositionPass { started, cursor }
    }

    /// `text` with the preedit spliced in at `caret`: the caret lands at the input
    /// method's `cursor`, and the composition is the washed range.
    pub(super) fn splice(&mut self, text: &str, caret: usize, cursor: usize) -> Shown<'_> {
        self.display.clear();
        self.display.push_str(&text[..caret]);
        self.display.push_str(&self.preedit);
        self.display.push_str(&text[caret..]);
        Shown {
            text: &self.display,
            caret: caret + cursor,
            wash: Some(caret..caret + self.preedit.len()),
        }
    }
}
