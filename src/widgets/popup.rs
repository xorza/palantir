use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::tree::Layer;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::theme::Surface;

/// What happens when the user presses outside the popup's body.
///
/// Both modes install a full-surface "click-eater" leaf in the
/// `Popup` layer behind the popup body — outside clicks hit the
/// eater (`Sense::CLICK`) and don't propagate to the `Main` tree
/// underneath. They differ only in whether the popup widget signals
/// dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal. Use for
///   confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — eater consumes the click AND
///   `PopupResponse.dismissed` is set so the host can flip its open
///   flag. Use for dropdowns, context menus, autocomplete.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    Block,
    Dismiss,
}

/// Result of [`Popup::show`]. `body` reflects the popup container's
/// own `Response` (pass-through of the wrapping `Panel`'s state).
/// `dismissed` is set when an outside click was eaten this frame and
/// the popup was configured for [`ClickOutside::Dismiss`]; hosts
/// read it to flip their open flag in the same frame.
pub struct PopupResponse {
    pub body: Response,
    pub dismissed: bool,
}

/// A side-layer container anchored to a screen rect. Records into
/// [`Layer::Popup`] so it draws above all `Main` siblings, escapes
/// ancestor clip, and hit-tests on top.
///
/// `anchor` is a caller-supplied screen rect — typically a trigger
/// widget's last-frame `Response.state.rect`. Mid-recording is
/// supported (the popup may be invoked from inside an open
/// `Panel::show` body); the reorder pass in `Tree::end_frame`
/// compacts records into layer-sorted contiguous storage.
///
/// Outside clicks are handled per [`ClickOutside`]: a full-surface
/// "click-eater" leaf is recorded in the `Popup` layer underneath
/// the body, so clicks anywhere outside the body don't leak through
/// to the `Main` tree. Inside-body clicks route to the body's own
/// leaves first (popup hit-test priority).
pub struct Popup {
    anchor: Rect,
    surface: Option<Surface>,
    padding: f32,
    click_outside: ClickOutside,
    /// Optional caller-supplied stable seed for derived `WidgetId`s
    /// (eater + body). Lets multiple simultaneous popups coexist
    /// without ID collisions; defaults to a seed derived from the
    /// anchor rect.
    id_seed: Option<WidgetId>,
}

impl Popup {
    #[track_caller]
    pub fn anchored_to(anchor: Rect) -> Self {
        Self {
            anchor,
            surface: None,
            padding: 0.0,
            click_outside: ClickOutside::Dismiss,
            id_seed: None,
        }
    }

    pub fn background(mut self, s: impl Into<Surface>) -> Self {
        self.surface = Some(s.into());
        self
    }

    pub fn padding(mut self, p: f32) -> Self {
        self.padding = p;
        self
    }

    pub fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Override the auto-derived id base by hashing `key`. Use when
    /// multiple popups are open in the same frame and their default
    /// anchor-derived seeds would collide.
    pub fn id_salt(mut self, key: impl std::hash::Hash) -> Self {
        self.id_seed = Some(WidgetId::from_hash(key));
        self
    }

    /// Override the auto-derived id base with a precomputed seed.
    pub fn id(mut self, seed: WidgetId) -> Self {
        self.id_seed = Some(seed);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> PopupResponse {
        let surface_rect = ui.display.logical_rect();
        let seed = self
            .id_seed
            .unwrap_or_else(|| WidgetId::from_hash(("palantir.popup", self.anchor)));
        let eater_id = seed.with("eater");
        let body_id = seed.with("body");
        // Eater root: full-surface invisible `Sense::CLICK` leaf.
        // Records first in the `Popup` layer scope so it paints
        // *under* the body and (via reverse-iter hit-test) the
        // body's deeper leaves get visited first — only clicks
        // outside the body's rect fall through to the eater.
        ui.layer(Layer::Popup, surface_rect, |ui| {
            Frame::new()
                .id(eater_id)
                .size((Sizing::FILL, Sizing::FILL))
                .sense(Sense::CLICK)
                .show(ui);
        });
        // Body root: anchored at the trigger; outer wrap is a
        // `Panel` with `Sense::CLICK` so clicks landing on the body
        // background (gaps between leaves) are absorbed by the body
        // rather than falling through to the eater.
        let surface = self.surface;
        let padding = self.padding;
        let mut body_resp: Option<Response> = None;
        ui.layer(Layer::Popup, self.anchor, |ui| {
            let mut panel = Panel::vstack()
                .id(body_id)
                .padding(padding)
                .sense(Sense::CLICK);
            if let Some(s) = surface {
                panel = panel.background(s);
            }
            body_resp = Some(panel.show(ui, body));
        });
        let body = body_resp.expect("popup body did not record a root widget");
        // The eater fires `clicked` only when the press landed on
        // the eater's rect — i.e. outside the body. (Body's rect
        // sits on top of the eater in hit-test order.)
        let eater_clicked = ui.response_for(eater_id).clicked;
        let dismissed = eater_clicked && self.click_outside == ClickOutside::Dismiss;
        PopupResponse { body, dismissed }
    }
}
