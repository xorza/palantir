//! [`Ime`]: the input method's composition.

use crate::common::span::Span;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::ime_preedit::ImePreedit;

/// The input method's uncommitted text, its cursor, and the widget focused when it arrived.
#[derive(Debug, Default)]
pub(super) struct Ime {
    pub(super) text: String,
    pub(super) cursor: Option<Span>,
    /// The widget focused when the preedit arrived; a focus that settles elsewhere retires it.
    pub(super) owner: Option<WidgetId>,
}

impl Ime {
    /// Take `preedit` as the composition of `owner`, the widget focused now.
    pub(super) fn compose(&mut self, preedit: ImePreedit<'_>, owner: Option<WidgetId>) {
        self.text.clear();
        self.text.push_str(preedit.text);
        self.cursor = preedit.cursor;
        self.owner = owner;
    }

    /// End the composition its commit replaces, keeping the widget it was for.
    pub(super) fn end(&mut self) {
        self.text.clear();
        self.cursor = None;
    }

    /// Drop the composition and the widget it was for.
    pub(super) fn retire(&mut self) {
        self.end();
        self.owner = None;
    }

    /// The composition, while `focused` is the widget it arrived for.
    pub(super) fn shown_to(&self, focused: Option<WidgetId>) -> Option<ImePreedit<'_>> {
        (!self.text.is_empty() && self.owner.is_some() && self.owner == focused).then(|| {
            ImePreedit {
                text: &self.text,
                cursor: self.cursor,
            }
        })
    }
}
