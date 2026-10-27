//! Keyboard focus: who holds it, whether the keyboard put it there, and where it returns as an
//! overlay closes.

use crate::cascade::Cascade;
use crate::primitives::identity::widget_id::WidgetId;

/// What moved focus, which decides whether the focus ring shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FocusCause {
    /// Tab or an arrow group: the ring shows, as CSS `:focus-visible` does.
    Keyboard,
    /// A left press: the ring hides.
    Pointer,
    /// The app or a widget: the ring keeps its state.
    Program,
}

/// The focus holder and everything that decides where focus goes next. Moves land through
/// [`Self::move_to`] at once; [`Self::reconcile`] settles them against the frame's cascade.
#[derive(Debug, Default)]
pub(super) struct Focus {
    focused: Option<WidgetId>,
    /// Whether [`Self::focused`] came from the keyboard and so shows the focus ring.
    visible: bool,
    /// Where the last [`Self::reconcile`] left focus: what an entry into an overlay this frame
    /// returns to, whichever path moved it there.
    settled: Option<WidgetId>,
    /// One row per overlay focus is inside.
    returns: Vec<FocusReturn>,
    /// The topmost dialog root the last cascade held, to notice a new dialog.
    modal_root: Option<WidgetId>,
    /// The ancestor whose first Tab stop takes focus at the next reconcile.
    first_within: Option<WidgetId>,
}

#[derive(Clone, Copy, Debug)]
struct FocusReturn {
    overlay: WidgetId,
    to: WidgetId,
}

impl Focus {
    #[inline]
    pub(super) const fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    #[inline]
    pub(super) const fn visible(&self) -> bool {
        self.visible
    }

    pub(super) const fn move_to(&mut self, next: Option<WidgetId>, cause: FocusCause) {
        self.focused = next;
        match cause {
            FocusCause::Keyboard => self.visible = true,
            FocusCause::Pointer => self.visible = false,
            FocusCause::Program => {}
        }
    }

    pub(super) const fn focus_first_within(&mut self, ancestor: WidgetId) {
        self.first_within = Some(ancestor);
    }

    /// The focus a warmup pass records against: this holder, and nothing owed.
    pub(super) const fn warmup_scratch(&self) -> Self {
        Self {
            focused: self.focused,
            visible: false,
            settled: None,
            returns: Vec::new(),
            modal_root: None,
            first_within: None,
        }
    }

    /// Keep the moves a warmup pass made.
    pub(super) fn adopt_warmup(&mut self, warmup: &Self) {
        self.focused = warmup.focused;
        self.first_within = warmup.first_within.or(self.first_within);
    }

    /// Settle focus against the frame's final cascade: drop a holder that can no longer take
    /// focus, give focus back from closed overlays, move it into a new dialog or a requested
    /// subtree, and note where an overlay entry returns. Returns whether focus moved.
    pub(super) fn reconcile(&mut self, cascade: &Cascade) -> bool {
        let before = self.focused;
        // A holder that left the tree, or was disabled or hidden, drops focus, as HTML's focus
        // fixup and WPF's `IsEnabled` do; otherwise keystrokes route to a widget that answers none.
        if self.focused.is_some_and(|id| !cascade.is_focusable(id)) {
            self.focused = None;
        }
        self.return_from_closed_overlays(cascade);
        // A dialog takes focus as it becomes the topmost, like `<dialog>.showModal()`, unless focus
        // is inside it already: a dialog a nested one closed over got its focus back above.
        let modal_root = cascade.modal_root().map(|row| row.id);
        if let Some(root) = modal_root
            && modal_root != self.modal_root
            && !self.focused.is_some_and(|id| cascade.is_within(id, root))
        {
            self.first_within = modal_root;
        }
        self.modal_root = modal_root;
        if let Some(ancestor) = self.first_within.take()
            && let Some(first) = cascade.first_tab_stop_within(ancestor)
        {
            self.enter(ancestor, self.focused, cascade);
            self.focused = Some(first);
        }
        // Tab, a press or the app moved focus into an overlay this frame: it returns to where the
        // frame found it, as WAI-ARIA asks of a dialog or menu whatever opened it.
        if let Some(now) = self.focused
            && let Some(overlay) = cascade.overlay_root_of(now)
            && !self
                .returns
                .iter()
                .any(|row| cascade.is_within(now, row.overlay))
        {
            self.enter(overlay, self.settled, cascade);
        }
        self.settled = self.focused;
        self.focused != before
    }

    /// Record that focus entering `overlay` from `from` returns there, unless it is already inside.
    fn enter(&mut self, overlay: WidgetId, from: Option<WidgetId>, cascade: &Cascade) {
        let Some(from) = from else {
            return;
        };
        let inside = cascade.is_within(from, overlay)
            || self.returns.iter().any(|row| row.overlay == overlay);
        if !inside {
            self.returns.push(FocusReturn { overlay, to: from });
        }
    }

    /// Give focus back, newest first, for every overlay that left the cascade, if focus is now
    /// nowhere and the holder remains.
    fn return_from_closed_overlays(&mut self, cascade: &Cascade) {
        while let Some(idx) = self
            .returns
            .iter()
            .rposition(|row| !cascade.by_id.contains_key(&row.overlay))
        {
            let row = self.returns.remove(idx);
            if self.focused.is_none() && cascade.is_focusable(row.to) {
                self.focused = Some(row.to);
            }
        }
    }
}
