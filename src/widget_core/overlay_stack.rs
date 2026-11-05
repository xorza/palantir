//! The overlays recording now, innermost last, the close a row in each asked for, and the cancel
//! a modal that opened asks of the light-dismiss overlays outside it.

use std::mem;

use crate::primitives::identity::widget_id::WidgetId;

/// One entry per overlay body recording now, innermost last: its owner, and whether a row in it
/// asked to close it. A [`MenuItem`](crate::MenuItem) closes the overlay it sits in through it, so
/// a row takes no handle.
///
/// A modal that opens cancels every light-dismiss overlay open before it, as Windows sends
/// `WM_CANCELMODE` to end a menu's mode when a dialog activates, and macOS ends menu tracking
/// before it shows a modal. A press outside closes such an overlay, but a modal can open with no
/// press: a close the window manager asks for, a file the system hands over, the app's own code.
/// Left open, it would sit above the modal's scrim on its own layer, and its barrier would take
/// the first press meant for the dialog. The overlays the modal opened inside are kept, as closing
/// one would close the modal with it, and so is every overlay that opened in the pass the modal
/// did, which was not open before it.
///
/// The cancel applies at once to the overlays that record after the modal. One that already
/// recorded in the pass cannot be taken back, so it alone asks for the next pass
/// ([`Self::needs_pass`]). An app that records its modal first takes no second pass.
///
/// Capacity retained, so no pass allocates after the deepest nesting.
#[derive(Debug, Default)]
pub(crate) struct OverlayStack {
    entered: Vec<Entered>,
    /// Boxed, as `Ui` keeps this stack inline and every pass walks `Ui`
    /// (`internals::hot_struct_sizes`), while a cancel is rare.
    cancel: Box<Cancel>,
}

/// The cancel a modal that opened asks for, from the pass it arms to the pass it applies.
#[derive(Debug, Default)]
struct Cancel {
    /// The overlays that opened this pass.
    opened: Vec<WidgetId>,
    /// The light-dismiss overlays open before this pass that recorded in it uncancelled.
    recorded: Vec<WidgetId>,
    /// The overlays a cancel armed this pass keeps: the modal that opened and those it opened
    /// inside. It applies at once, and to the next pass too when [`OverlayStack::needs_pass`],
    /// which adds those that opened in this one.
    armed: Option<Vec<WidgetId>>,
    /// The cancel this pass applies, and the overlays it keeps.
    active: Option<Vec<WidgetId>>,
    /// The buffer neither cancel holds, kept for its capacity.
    spare: Vec<WidgetId>,
}

/// One overlay body recording now.
#[derive(Clone, Copy, Debug)]
struct Entered {
    owner: WidgetId,
    close: bool,
}

impl OverlayStack {
    /// A record pass opens: the cancel armed last pass applies to this one if an overlay it
    /// closes had already recorded there, and the one it applied ends.
    pub(crate) fn begin_pass(&mut self) {
        debug_assert!(
            self.entered.is_empty(),
            "every overlay left the pass before"
        );
        let stale = self.needs_pass();
        let cancel = &mut *self.cancel;
        if let Some(mut ended) = cancel.active.take() {
            ended.clear();
            cancel.spare = ended;
        }
        if let Some(mut keep) = cancel.armed.take() {
            if stale {
                keep.extend_from_slice(&cancel.opened);
                cancel.active = Some(keep);
            } else {
                keep.clear();
                cancel.spare = keep;
            }
        }
        cancel.opened.clear();
        cancel.recorded.clear();
    }

    /// The body of the overlay `owner` starts recording, opening this pass when `opened`.
    pub(crate) fn enter(&mut self, owner: WidgetId, opened: bool) {
        if opened {
            self.cancel.opened.push(owner);
        }
        self.entered.push(Entered {
            owner,
            close: false,
        });
    }

    /// The light-dismiss overlay `owner`, open before this pass, records in it uncancelled.
    pub(crate) fn note_recorded(&mut self, owner: WidgetId) {
        self.cancel.recorded.push(owner);
    }

    /// The innermost overlay body ends; whether a row in it asked to close it.
    pub(crate) fn leave(&mut self) -> bool {
        self.entered
            .pop()
            .expect("an overlay leaves only after it entered")
            .close
    }

    /// Ask the innermost overlay recording now to close. No-op outside every overlay, as a row
    /// recorded in the main tree has nothing to close.
    pub(crate) fn close_innermost(&mut self) {
        if let Some(entered) = self.entered.last_mut() {
            entered.close = true;
        }
    }

    /// The modal `owner` opened: arm the cancel, keeping `owner` and every overlay recording
    /// around it now. A second modal opening in the pass keeps its own too.
    pub(crate) fn arm_cancel(&mut self, owner: WidgetId) {
        let cancel = &mut *self.cancel;
        let keep = cancel
            .armed
            .get_or_insert_with(|| mem::take(&mut cancel.spare));
        keep.push(owner);
        keep.extend(self.entered.iter().map(|entered| entered.owner));
    }

    /// Whether a cancel armed this pass closes an overlay that already recorded in it, which
    /// only the next pass can take away.
    pub(crate) fn needs_pass(&self) -> bool {
        let Cancel {
            recorded, armed, ..
        } = &*self.cancel;
        armed
            .as_ref()
            .is_some_and(|keep| recorded.iter().any(|owner| !keep.contains(owner)))
    }

    /// Whether a cancel closes the light-dismiss overlay `owner`, open before it, when it records
    /// now: every one but those the opening modal kept.
    pub(crate) fn cancels(&self, owner: WidgetId) -> bool {
        let Cancel { armed, active, .. } = &*self.cancel;
        [armed, active]
            .into_iter()
            .flatten()
            .any(|keep| !keep.contains(&owner))
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::widget_core::overlay_stack::OverlayStack;

    /// A close reaches the innermost overlay alone; one asked outside every overlay is dropped.
    #[test]
    fn a_close_reaches_the_innermost_overlay() {
        let [outer, inner] = [1, 2].map(WidgetId::from_hash);
        let mut stack = OverlayStack::default();
        stack.close_innermost();
        stack.enter(outer, false);
        stack.enter(inner, false);
        stack.close_innermost();
        assert!(stack.leave(), "the inner overlay closes");
        assert!(!stack.leave(), "the outer one stays open");
    }

    /// A cancel applies at once to what records after the modal. It keeps the modal and the
    /// overlays around it. With no overlay it closes already recorded, the next pass cancels
    /// nothing. An overlay recorded before it asks for the next pass, which closes that overlay,
    /// keeps those that opened in the modal's pass, and is the last pass the cancel holds.
    #[test]
    fn a_cancel_applies_at_once_and_reaches_the_next_pass_only_for_a_stale_overlay() {
        let [container, modal, before, beside, after] = [1, 2, 3, 4, 5].map(WidgetId::from_hash);

        let mut stack = OverlayStack::default();
        stack.begin_pass();
        stack.note_recorded(container);
        stack.enter(container, false);
        stack.arm_cancel(modal);
        assert!(!stack.leave());
        assert!(stack.cancels(after), "at once, for what records after");
        assert!(!stack.cancels(modal), "the modal stays");
        assert!(!stack.cancels(container), "its container stays");
        assert!(!stack.needs_pass(), "a kept container is not stale");
        stack.begin_pass();
        assert!(!stack.cancels(after), "nothing stale, so nothing carries");

        let mut stack = OverlayStack::default();
        stack.begin_pass();
        assert!(!stack.needs_pass(), "nothing armed");
        stack.note_recorded(before);
        assert!(!stack.cancels(before), "nothing armed");
        assert!(!stack.needs_pass(), "nothing armed");
        stack.arm_cancel(modal);
        stack.enter(beside, true);
        assert!(!stack.leave());
        assert!(stack.needs_pass(), "the overlay already recorded");
        stack.begin_pass();
        assert!(stack.cancels(before), "the pass after");
        assert!(!stack.cancels(modal));
        assert!(!stack.cancels(beside), "one that opened beside it stays");
        assert!(!stack.needs_pass(), "only the pass that armed it asks");
        stack.begin_pass();
        assert!(!stack.cancels(before), "one pass only");
    }
}
