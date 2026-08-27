//! The centred dialog and its input-blocking backdrop, plus what a frame
//! of it reports about dismissal.

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::placement::Placement;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::scene::node::theme_defaults::ThemeDefaults;
use crate::ui::Ui;
use crate::widgets::overlay_scope::OverlayScope;
use crate::widgets::theme::modal::ModalTheme;
use glam::Vec2;

/// A centered dialog over a dimming, input-blocking backdrop, recorded
/// into [`Layer::Modal`] so it draws above everything and hit-tests
/// first. The card hugs its content (floored at a min width) and centers
/// on the surface.
///
/// Dismissal: clicking the backdrop (anywhere outside the card) or
/// pressing Esc sets [`ModalResponse::dismissed`] — the host flips its
/// own open flag. Clicks on the card itself are absorbed, so interacting
/// with dialog content never closes it.
#[derive(Debug)]
pub struct Modal<'a> {
    node: Node,
    chrome: Option<Background>,
    backdrop: Option<Color>,
    style: Option<&'a ModalTheme>,
}

/// Outcome of [`Modal::show`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ModalResponse {
    /// The backdrop was clicked, or Esc was pressed, this frame.
    pub dismissed: bool,
}

impl<'a> Modal<'a> {
    #[track_caller]
    pub fn new() -> Self {
        let mut node = Node::vstack();
        node.flags.set_sense(Sense::ABSORB_POINTER);
        Self {
            node,
            chrome: None,
            backdrop: None,
            style: None,
        }
    }

    style_setter!(
        'a,
        ModalTheme,
        modal,
        "Per-field [`Self::background`] / [`Self::backdrop`] still win over it.",
    );

    /// Backdrop scrim color, defaulting to [`crate::Theme::modal`]'s.
    /// One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn backdrop(mut self, c: Color) -> Self {
        self.backdrop = Some(c);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> ModalResponse {
        let surface = ui.display().logical_rect();
        // The caller's salt names the *backdrop root*, but the node it
        // arrived on is the card — the root is framework-built below,
        // out of the id itself, so identity resolves on its own and the
        // root is staged onto it once it exists.
        let mut root_w = ui.widget(self.node);
        let root_id = root_w.id();

        // Handle: `mt.card` is still borrowed at `scope.record`, which owns
        // `ui` mutably.
        let ui_theme = ui.theme().clone();
        let mt = self.slot(&ui_theme);
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let card_bg = self.chrome.as_ref().unwrap_or(&mt.card);
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        // The card's own id is always derived — the caller's went to the
        // root — so this is `id`, not `default_id`.
        let card = self
            .node
            .id(root_id.with("card"))
            .default_padding(theme_padding)
            .default_min_size(Size::new(theme_min_width, 0.0));

        // Root fills the surface, dims it, eats stray pointer events,
        // and centers the card. The card re-senses `Sense::ABSORB_POINTER` so clicks
        // on it never fall through to this dismiss-backdrop.
        let mut root = Node::zstack()
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .sense(Sense::ABSORB_POINTER);
        let placement = Placement::fixed(Vec2::ZERO, Some(surface.size));
        let scope = OverlayScope::claim(root_id, Layer::Modal, placement, &mut root);
        // The backdrop root displaces the card the salt arrived on —
        // after `claim`, which writes the placement into it.
        root_w.node = root;
        let escape = scope.record(ui, |ui| {
            root_w.record(ui, Some(&dim), |ui| {
                ui.widget(card).record(ui, Some(card_bg), body);
            });
        });
        let dismissed = ui.response_for(root_id).left.clicked() || escape;
        scope.withdraw(ui, dismissed);

        ModalResponse { dismissed }
    }
}

impl_background!(
    Modal<'_>,
    "The card chrome. Pass [`Background::NONE`] to suppress the themed card \
     chrome for this modal.",
);
impl_configure!(Modal<'_>);

#[cfg(test)]
mod tests;
