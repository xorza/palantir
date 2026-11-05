//! The overlays recording now, innermost last, the close a row in each asked for, and the cancel
//! a modal that opened asks of the light-dismiss overlays outside it.

use std::mem;

use crate::primitives::identity::widget_id::WidgetId;

/// One entry per overlay body recording now, innermost last: its owner, and whether a row in it
/// asked to close it. A [`MenuItem`](crate::MenuItem) closes the overlay it sits in through it, so
/// a row takes no handle.
///
/// A modal that opens cancels every light-dismiss overlay open before it on the next pass, as
/// Windows sends `WM_CANCELMODE` to end a menu's mode when a dialog activates, and macOS ends menu
/// tracking before it shows a modal. A press outside closes such an overlay, but a modal can open
/// with no press: a close the window manager asks for, a file the system hands over, the app's own
/// code. Left open, it would sit above the modal's scrim on its own layer, and its barrier would
/// take the first press meant for the dialog. The overlays the modal opened inside are kept, as
/// closing one would close the modal with it, and so is every overlay that opened in the pass
/// the modal did, which was not open before it.
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
    /// The overlays a cancel armed this pass keeps: the modal that opened and those it opened
    /// inside; the next pass adds those that opened in this one, and applies it.
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
    /// A record pass opens: the cancel armed last pass applies to this one, and the one it
    /// applied ends.
    pub(crate) fn begin_pass(&mut self) {
        debug_assert!(
            self.entered.is_empty(),
            "every overlay left the pass before"
        );
        let cancel = &mut *self.cancel;
        if let Some(mut ended) = cancel.active.take() {
            ended.clear();
            cancel.spare = ended;
        }
        cancel.active = cancel.armed.take().map(|mut keep| {
            keep.extend_from_slice(&cancel.opened);
            keep
        });
        cancel.opened.clear();
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

    /// The modal `owner` opened: arm the cancel the next pass applies, keeping `owner` and every
    /// overlay recording around it now. A second modal opening in the pass keeps its own too.
    pub(crate) fn arm_cancel(&mut self, owner: WidgetId) {
        let cancel = &mut *self.cancel;
        let keep = cancel
            .armed
            .get_or_insert_with(|| mem::take(&mut cancel.spare));
        keep.push(owner);
        keep.extend(self.entered.iter().map(|entered| entered.owner));
    }

    /// Whether a modal that opened this pass armed a cancel, which the next pass applies.
    pub(crate) fn is_armed(&self) -> bool {
        self.cancel.armed.is_some()
    }

    /// Whether the cancel this pass applies closes the light-dismiss overlay `owner`, open
    /// before it: every one but those the opening modal kept.
    pub(crate) fn cancels(&self, owner: WidgetId) -> bool {
        self.cancel
            .active
            .as_ref()
            .is_some_and(|keep| !keep.contains(&owner))
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

    /// A cancel armed in one pass applies to the next pass alone, and keeps the modal, the
    /// overlays around it when it opened, and those that opened in its pass: a popup the modal
    /// opened inside of stays, one opened beside it in its pass stays, one open before it closes.
    /// A pass with nothing armed cancels nothing.
    #[test]
    fn a_cancel_applies_to_the_next_pass_and_keeps_the_modal_and_its_containers() {
        let [container, modal, before, beside] = [1, 2, 3, 4].map(WidgetId::from_hash);
        let mut stack = OverlayStack::default();
        stack.begin_pass();
        stack.enter(before, false);
        assert!(!stack.leave());
        stack.enter(container, false);
        stack.arm_cancel(modal);
        assert!(!stack.leave());
        stack.enter(beside, true);
        assert!(!stack.leave());
        assert!(!stack.cancels(before), "not in the pass that armed it");

        stack.begin_pass();
        assert!(stack.cancels(before), "the pass after");
        assert!(!stack.cancels(modal), "the modal stays");
        assert!(!stack.cancels(container), "its container stays");
        assert!(!stack.cancels(beside), "one that opened beside it stays");

        stack.begin_pass();
        assert!(!stack.cancels(before), "one pass only");
    }
}
