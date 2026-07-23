use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::scene::element::{Configure, ConfigureElement, Element};
use crate::scene::layer::Layer;
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
#[derive(Debug)]
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
        let mut element = Element::vstack();
        element.flags.set_sense(BLOCK);
        Self {
            element,
            chrome: None,
            backdrop: None,
        }
    }

    /// Override the card chrome (fill / stroke / corners / shadow). Pass
    /// [`Background::NONE`] to suppress the themed card chrome.
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
        let mut widget = ui.widget(self.element);
        let root_id = widget.id();

        let mt = &ui.theme.modal;
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let card_bg = self.chrome.unwrap_or_else(|| mt.card.clone());
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        // The user-configured element becomes the card; the widget's
        // resolved id stays on the backdrop root it records below.
        let mut card = widget.element.id(root_id.with("card"));
        card.padding.get_or_insert(theme_padding);
        card.min_size.get_or_insert(Size::new(theme_min_width, 0.0));

        // Root fills the surface, dims it, eats stray pointer events,
        // and centers the card. The card re-senses `BLOCK` so clicks
        // on it never fall through to this dismiss-backdrop.
        widget.element = Element::zstack()
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .sense(BLOCK);
        ui.layer(Layer::Modal, Vec2::ZERO, Some(surface.size), |ui| {
            widget.node(ui, Some(&dim), |ui| {
                ui.widget(card).node(ui, Some(&card_bg), body);
            });
        });

        ModalResponse {
            dismissed: ui.response_for(root_id).left.clicked() || ui.escape_pressed(),
        }
    }
}

impl Configure for Modal {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::Ui;
    use crate::primitives::background::Background;
    use crate::primitives::size::Size;
    use crate::primitives::spacing::Spacing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::element::Configure;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node::NodeId;
    use crate::widgets::modal::Modal;
    use glam::UVec2;

    #[test]
    fn explicit_zero_padding_and_minimum_override_card_theme() {
        let mut ui = Ui::for_test();
        let root_id = WidgetId::from_hash("modal-explicit-zero");
        ui.run_at_without_baseline(UVec2::new(400, 300), |ui| {
            Modal::new()
                .id(root_id)
                .background(Background::NONE)
                .padding(Spacing::ZERO)
                .min_size(Size::ZERO)
                .show(ui, |_| {});
        });

        let card_id = root_id.with("card");
        let tree = &ui.forest.trees[Layer::Modal];
        let index = tree
            .records
            .widget_id()
            .iter()
            .position(|id| *id == card_id)
            .expect("modal card node");
        let node = NodeId(index as u32);
        assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
        assert_eq!(tree.bounds(node).min_size, Size::ZERO);
    }
}
