use crate::input::keyboard::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
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
pub struct Modal {
    node: Node,
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
        let mut node = Node::vstack();
        node.flags.set_sense(Sense::ABSORB_POINTER);
        Self {
            node,
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
        let mut widget = ui.widget(self.node);
        let root_id = widget.id();

        let mt = &ui.theme.modal;
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let card_bg = self.chrome.unwrap_or_else(|| mt.card.clone());
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        // The user-configured node becomes the card; the widget's
        // resolved id stays on the backdrop root it records below.
        let mut card = widget.node.id(root_id.with("card"));
        card.padding.get_or_insert(theme_padding);
        card.min_size.get_or_insert(Size::new(theme_min_width, 0.0));

        // Root fills the surface, dims it, eats stray pointer events,
        // and centers the card. The card re-senses `Sense::ABSORB_POINTER` so clicks
        // on it never fall through to this dismiss-backdrop.
        widget.node = Node::zstack()
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .sense(Sense::ABSORB_POINTER);
        // A modal *owns* the keyboard, it does not merely outrank the
        // popups below it. Claiming capture on `Layer::Modal` makes that
        // explicit: the topmost claimant wins, so a popup underneath stops
        // seeing Escape entirely instead of dismissing alongside the
        // modal, and the modal's own body still reads the raw stream
        // because a capture never silences its own layer.
        let escape = ui.layer(Layer::Modal, Vec2::ZERO, Some(surface.size), |ui| {
            let keyboard = ui.claim_keyboard(root_id);
            widget.record(ui, Some(&dim), |ui| {
                ui.widget(card).record(ui, Some(&card_bg), body);
            });
            keyboard.key_pressed(ui, Shortcut::key(Key::Escape))
        });

        ModalResponse {
            dismissed: ui.response_for(root_id).left.clicked() || escape,
        }
    }
}

impl Configure for Modal {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::Ui;
    use crate::input::InputEvent;
    use crate::input::keyboard::Key;
    use crate::primitives::background::Background;
    use crate::primitives::size::Size;
    use crate::primitives::spacing::Spacing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::tree::node::NodeId;
    use crate::widgets::modal::Modal;
    use crate::widgets::popup::Popup;
    use glam::{UVec2, Vec2};

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

    /// A modal paints above every popup and eats pointer input through
    /// its backdrop, so it must also be the one that hears Escape. It
    /// previously could not: any popup holding keyboard capture emptied
    /// the uncaptured stream the modal read from, leaving it
    /// undismissable for as long as the popup stayed open.
    ///
    /// The control matters as much as the case — with no popup open the
    /// modal has always dismissed, so asserting only the popup case
    /// would not distinguish "layer ordering works" from "Escape works".
    #[test]
    fn modal_hears_escape_even_while_a_popup_below_holds_keyboard_capture() {
        fn escape_dismisses(with_popup: bool) -> bool {
            const SURFACE: UVec2 = UVec2::new(400, 300);
            let scene = |ui: &mut Ui, dismissed: &mut bool| {
                if with_popup {
                    Popup::anchored_to(Vec2::ZERO)
                        .id(WidgetId::from_hash("under-modal"))
                        .show(ui, |_ui, _handle| {});
                }
                *dismissed |= Modal::new()
                    .id(WidgetId::from_hash("modal-escape"))
                    .show(ui, |_| {})
                    .dismissed;
            };

            // Two frames: the keyboard wake-gate parks a press whose
            // shortcut nobody subscribed yet, so the first frame is what
            // registers the modal's interest in Escape and the press lands
            // after it.
            //
            // `|=` rather than `=` because a frame may record twice, and
            // `post_record` drains the key queue between passes — so pass B
            // sees no Escape and would overwrite pass A's result. Real
            // callers have the same shape: they flip an `open` flag on
            // dismissal rather than reading the last pass's return.
            let mut ui = Ui::for_test();
            let mut dismissed = false;
            ui.run_at(SURFACE, |ui| scene(ui, &mut dismissed));
            ui.on_input(InputEvent::KeyDown {
                key: Key::Escape,
                repeat: false,
                physical: Key::Escape,
            });
            let mut dismissed = false;
            ui.run_at_without_baseline(SURFACE, |ui| scene(ui, &mut dismissed));
            dismissed
        }

        assert!(
            escape_dismisses(false),
            "control: a modal with no popup open must dismiss on Escape",
        );
        assert!(
            escape_dismisses(true),
            "a popup's keyboard capture must not silence the modal above it",
        );
    }

    /// Exactly one overlay may act on a given Escape.
    ///
    /// Layer-ordered *reads* alone were not enough: the modal saw Escape
    /// and so did the popup that held capture, so one keypress closed
    /// both. Ownership is now resolved topmost-first, so the modal takes
    /// the keyboard and the popup beneath it sees nothing at all.
    #[test]
    fn escape_closes_only_the_topmost_overlay() {
        const SURFACE: UVec2 = UVec2::new(400, 300);
        let scene = |ui: &mut Ui, modal: &mut bool, popup: &mut bool| {
            *popup |= Popup::anchored_to(Vec2::ZERO)
                .id(WidgetId::from_hash("under-modal"))
                .show(ui, |_ui, _handle| {})
                .dismissed;
            *modal |= Modal::new()
                .id(WidgetId::from_hash("over-popup"))
                .show(ui, |_| {})
                .dismissed;
        };

        let mut ui = Ui::for_test();
        let (mut m, mut p) = (false, false);
        ui.run_at(SURFACE, |ui| scene(ui, &mut m, &mut p));
        ui.on_input(InputEvent::KeyDown {
            key: Key::Escape,
            repeat: false,
            physical: Key::Escape,
        });
        let (mut modal_closed, mut popup_closed) = (false, false);
        ui.run_at_without_baseline(SURFACE, |ui| {
            scene(ui, &mut modal_closed, &mut popup_closed)
        });

        assert!(modal_closed, "the topmost overlay must take the Escape");
        assert!(
            !popup_closed,
            "the popup beneath the modal must not also consume it",
        );
    }
}
