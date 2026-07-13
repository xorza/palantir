use crate::forest::Layer;
use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::ui::Ui;
use crate::widgets::frame::Frame;
use crate::widgets::resolve_container_chrome;
use glam::Vec2;
use std::cell::Cell;

/// What happens when the user presses outside the popup's body.
///
/// Both modes install a full-surface "click-eater" leaf in the
/// `Popup` layer behind the popup body — outside presses hit the
/// eater (it senses `CLICK | DRAG | SCROLL | PINCH`) and don't
/// propagate to the `Main` tree underneath. They differ only in
/// whether the popup widget signals
/// dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal (and Esc is
///   ignored). Use for confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — an eaten outside-click **or** an Esc press sets
///   `PopupResponse.dismissed` so the host can flip its open flag. Use for
///   dropdowns, context menus, autocomplete.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
}

/// Per-frame close signal handed to the body closure. Content
/// widgets (e.g. [`crate::widgets::context_menu::MenuItem`]) call
/// [`Self::close`] to ask the enclosing popup to dismiss without
/// threading state through their caller.
///
/// Lives on the stack for the duration of one [`Popup::show`] call —
/// no ambient `Ui` state, no nested-popup signal-leak.
pub struct PopupHandle {
    requested: Cell<bool>,
}

impl PopupHandle {
    fn new() -> Self {
        Self {
            requested: Cell::new(false),
        }
    }

    /// Ask the enclosing popup to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
    }
}

/// Result of [`Popup::show`]. `dismissed` is set when a
/// [`ClickOutside::Dismiss`] popup is dismissed this frame — an eaten
/// outside-press or an Esc press. `close_requested` is set when a
/// content widget inside the body called [`PopupHandle::close`].
/// Hosts read either to flip their open flag in the same frame.
#[derive(Copy, Clone, Debug, Default)]
pub struct PopupResponse {
    pub dismissed: bool,
    pub close_requested: bool,
}

impl PopupResponse {
    /// `true` when the popup asked to close this frame — either an
    /// outside click dismissed it ([`Self::dismissed`]) or a content
    /// widget called [`PopupHandle::close`] ([`Self::close_requested`]).
    /// The single close-signal predicate shared by overlay-trigger
    /// widgets (`ComboBox`, `ContextMenu`) so the dismiss contract lives
    /// in one place.
    pub fn closed(&self) -> bool {
        self.dismissed || self.close_requested
    }
}

/// A side-layer container placed at a screen-space point. Records
/// into [`Layer::Popup`] so it draws above all `Main` siblings,
/// escapes ancestor clip, and hit-tests on top.
///
/// `anchor` is the body's top-left, typically derived from a trigger
/// widget's last-frame `Response.state.rect` (e.g. its bottom-left
/// for a dropdown). Sizing is governed by the body's own `Sizing`
/// chain — `Hug` shrinks to content, `Fill` fills the remaining
/// surface, `Fixed` is exact. Mid-recording is supported.
///
/// Outside clicks are handled per [`ClickOutside`]: a full-surface
/// "click-eater" leaf is recorded in the `Popup` layer underneath
/// the body, so clicks anywhere outside the body don't leak through
/// to the `Main` tree. Inside-body clicks route to the body's own
/// leaves first (popup hit-test priority).
///
/// Implements [`Configure`] — use `.id(...)`, `.id_salt(...)`,
/// `.padding(...)`, `.size(...)`, etc. on the popup body.
pub struct Popup {
    anchor: Vec2,
    click_outside: ClickOutside,
    element: Element,
    chrome: Option<Background>,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(anchor: Vec2) -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.flags.set_sense(Sense::CLICK);
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            element,
            chrome: None,
        }
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None`
    /// is the default; theme fallback in [`Self::show`] fills it in
    /// from `ui.theme.panel_background` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &PopupHandle)) -> PopupResponse {
        // Popup body resolves at the root of `Layer::Popup` (no
        // open frames in that layer), so `widget_id`'s
        // parent-scoping is a no-op — `body_id` equals the bare
        // salt hash. That keeps the eater id (and any persistent
        // popup-side state) stable regardless of where in `Main`
        // the trigger lives.
        let body_id = ui.widget_id(&self.element);
        let eater_id = body_id.with("eater");
        // Smart placement: read last frame's body size and place the
        // popup so it fits inside the surface. First open has no prior
        // rect — record at the raw anchor and request a relayout so
        // pass B places against the just-measured size.
        let surface = ui.display().logical_rect();
        let prev_size = ui.response_for(body_id).rect.map(|r| r.size);
        let placed = place_anchor(self.anchor, prev_size, surface);
        let first_open = prev_size.is_none();
        // Eater records first → paints under the body. Hit-test runs
        // reverse-iter so the body's leaves still win inside its rect.
        //
        // Senses all four pointer interactions so the popup is truly
        // modal-over-`Main`: pan-drag, scroll, and pinch over the
        // surrounding area can't leak through to the host (e.g. a
        // graph canvas underneath that pans on middle-drag and zooms
        // on scroll/pinch). `Sense::CLICK` is the dismiss trigger;
        // the other three never produce visible behavior on the
        // eater itself — they're absorbed and discarded so the host
        // doesn't see them.
        ui.layer(Layer::Popup, Vec2::ZERO, None, |ui| {
            Frame::new()
                .id(eater_id)
                .size((Sizing::FILL, Sizing::FILL))
                .sense(Sense::CLICK | Sense::DRAG | Sense::SCROLL | Sense::PINCH)
                .show(ui);
        });
        let mut element = self.element;
        let chrome = resolve_container_chrome(
            &mut element,
            self.chrome,
            ui.theme.panel_background.as_ref(),
            ui.theme.panel_clip,
        );
        // Anchor-independent measure — pass `Some(surface.size)` so
        // the body measures against its full natural height regardless
        // of where it'll paint. Without this, a popup anchored near
        // the bottom would be limited to `surface − anchor` (a few
        // pixels), measure into that slot, and `place_anchor` would
        // see the squeezed size and decide no flip is needed — feedback
        // loop that never lets the popup escape the bottom band.
        let handle = PopupHandle::new();
        let measure_cap = Some(surface.size);
        ui.layer(Layer::Popup, placed, measure_cap, |ui| {
            ui.node(body_id, element, chrome.as_ref(), |ui| body(ui, &handle));
        });
        if first_open {
            // No measured size yet → `placed` fell back to the raw
            // anchor. Run another pass so the just-measured size
            // feeds back into the placement algorithm.
            ui.request_relayout();
        }
        let dismiss_mode = self.click_outside == ClickOutside::Dismiss;
        let eater_clicked = ui.response_for(eater_id).clicked;
        PopupResponse {
            // A `Dismiss` popup closes on an eaten outside-press OR an Esc
            // press — so overlay hosts (ComboBox / ContextMenu) read one
            // `closed()` signal instead of each re-deriving Esc. (`Block`
            // short-circuits, so it neither dismisses on nor subscribes Esc.)
            dismissed: dismiss_mode && (eater_clicked || ui.escape_pressed()),
            close_requested: handle.requested.get(),
        }
    }
}

/// Pick a screen-space top-left for a popup body that prefers the
/// raw anchor but flips to the opposite side of it when the body
/// wouldn't fit on that side. After flipping, clamps to the surface
/// as a last-resort safety net so tall popups near the edge stay
/// visible (even if they slightly overlap the anchor).
///
/// Returns the raw `anchor` unchanged when `size` is `None` (no prior
/// frame to measure against). `Popup::show` pairs this with a one-shot
/// relayout request so the second pass places against measured size.
pub(crate) fn place_anchor(anchor: Vec2, size: Option<Size>, surface: Rect) -> Vec2 {
    let Some(s) = size else {
        return anchor;
    };
    let surface_max = surface.max();
    Vec2::new(
        place_axis(anchor.x, s.w, surface.min.x, surface_max.x),
        place_axis(anchor.y, s.h, surface.min.y, surface_max.y),
    )
}

/// Single-axis flip-then-clamp. `anchor` is the desired top/left edge,
/// `size` the body's extent on that axis. If the body would spill past
/// `surface_max` AND has room on the opposite side, flip so the
/// body's trailing edge lands on `anchor` instead. Then clamp so the
/// top-left stays on-surface even when the body's bigger than the
/// surface (trailing edge may still overflow — unavoidable).
fn place_axis(anchor: f32, size: f32, surface_min: f32, surface_max: f32) -> f32 {
    let pos = if anchor + size > surface_max && anchor - size >= surface_min {
        anchor - size
    } else {
        anchor
    };
    let max_pos = (surface_max - size).max(surface_min);
    pos.min(max_pos).max(surface_min)
}

impl Configure for Popup {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

#[cfg(test)]
mod tests;
