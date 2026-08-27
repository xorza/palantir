use glam::Vec2;

/// One button's drag lifecycle, carried on [`ButtonState::drag`](crate::ButtonState::drag) — the
/// owning button is the slot's position in [`ResponseState`](crate::ResponseState). The four
/// phases are mutually exclusive per button, which is why this is an
/// enum rather than an `Option` + edge flags:
///
/// `None` → `Started` (the threshold-crossing frame) → `Active` (every
/// following held frame) → `Stopped` (the release frame) → `None`.
///
/// `delta` is the cumulative pointer travel since press in pre-transform
/// widget-local logical coordinates. It is rect-independent; the pointer
/// may leave the widget's rect mid-drag and the delta keeps tracking.
/// `Stopped` carries no delta: the capture is already gone, so
/// commit-on-release gestures stash the running value while
/// `Started`/`Active` and commit it on `Stopped`.
///
/// A same-frame stop-and-relatch (release + press + threshold-crossing
/// move all in one event batch) reports the fresh `Started` — the new
/// gesture supersedes the stale stop edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Drag {
    /// No drag on this button: either nothing is pressed, or a press is
    /// down but hasn't travelled past the latch threshold yet.
    #[default]
    None,
    /// One-frame edge: the drag latched this frame. Snapshot anchors
    /// here.
    Started {
        /// Cumulative travel since press, widget-local and pre-transform.
        delta: Vec2,
    },
    /// Latched on an earlier frame, still held.
    Active {
        /// Cumulative travel since press, widget-local and pre-transform —
        /// not the per-frame increment.
        delta: Vec2,
    },
    /// One-frame edge: the latched drag ended this frame (release).
    Stopped,
}

impl Drag {
    /// Cumulative travel of a live drag (`Started` / `Active`).
    #[inline]
    pub fn delta(self) -> Option<Vec2> {
        match self {
            Drag::Started { delta } | Drag::Active { delta } => Some(delta),
            Drag::None | Drag::Stopped => None,
        }
    }

    /// A drag is live (`Started` / `Active`).
    #[inline]
    pub fn dragging(self) -> bool {
        matches!(self, Drag::Started { .. } | Drag::Active { .. })
    }

    /// One-frame edge: the latch frame.
    #[inline]
    pub fn started(self) -> bool {
        matches!(self, Drag::Started { .. })
    }

    /// One-frame edge: the release frame of a latched drag.
    #[inline]
    pub fn stopped(self) -> bool {
        matches!(self, Drag::Stopped)
    }
}
