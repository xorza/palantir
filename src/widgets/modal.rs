use crate::forest::Layer;
use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::ui::Ui;
use glam::Vec2;

/// Pointer senses absorbed by both the backdrop and the card, so no
/// click / drag / scroll / pinch leaks to the `Main` tree underneath.
const BLOCK: Sense = Sense::CLICK
    .union(Sense::DRAG)
    .union(Sense::SCROLL)
    .union(Sense::PINCH);

/// A centered dialog over a dimming, input-blocking backdrop, recorded
/// into [`Layer::Modal`] so it draws above everything and hit-tests
/// first. The card hugs its content (floored at a min width) and centers
/// on the surface.
///
/// Dismissal: clicking the backdrop (anywhere outside the card) or
/// pressing Esc sets [`ModalResponse::dismissed`] — the host flips its
/// own open flag. Clicks on the card itself are absorbed, so interacting
/// with dialog content never closes it.
pub struct Modal {
    element: Element,
    chrome: Option<Background>,
    backdrop: Option<Color>,
}

/// Outcome of [`Modal::show`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ModalResponse {
    /// The backdrop was clicked, or Esc was pressed, this frame.
    pub dismissed: bool,
}

impl Modal {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::VStack);
        element.flags.set_sense(BLOCK);
        Self {
            element,
            chrome: None,
            backdrop: None,
        }
    }

    /// Override the card chrome (fill / stroke / corners / shadow).
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    /// Override the backdrop scrim color. `None` (default) inherits
    /// [`crate::Theme::modal`]'s backdrop.
    pub fn backdrop(mut self, c: Color) -> Self {
        self.backdrop = Some(c);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> ModalResponse {
        let surface = ui.display().logical_rect();
        let root_id = ui.widget_id(&self.element);
        let card_id = root_id.with("card");

        let mt = &ui.theme.modal;
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let card_bg = self.chrome.unwrap_or_else(|| mt.card.clone());
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        let mut card = self.element;
        card.salt = Salt::Verbatim(card_id);
        if card.padding == Spacing::ZERO {
            card.padding = theme_padding;
        }
        if card.min_size.w <= 0.0 {
            card.min_size.w = theme_min_width;
        }

        ui.layer(Layer::Modal, Vec2::ZERO, Some(surface.size), |ui| {
            // Root fills the surface, dims it, eats stray pointer events,
            // and centers the card. The card re-senses `BLOCK` so clicks
            // on it never fall through to this dismiss-backdrop.
            let mut root = Element::new(LayoutMode::ZStack);
            root.salt = Salt::Verbatim(root_id);
            root.size = (Sizing::FILL, Sizing::FILL).into();
            root.child_align = Align::CENTER;
            root.flags.set_sense(BLOCK);
            ui.node(root_id, root, Some(&dim), |ui| {
                ui.node(card_id, card, Some(&card_bg), body);
            });
        });

        ModalResponse {
            dismissed: ui.response_for(root_id).clicked || ui.escape_pressed(),
        }
    }
}

impl Configure for Modal {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
