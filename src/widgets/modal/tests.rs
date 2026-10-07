use crate::Ui;
use crate::input::keyboard::key::Key;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::widget_core::configure::Configure;
use crate::widgets::modal::Modal;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};

#[test]
fn explicit_zero_padding_and_minimum_override_card_theme() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root_id = WidgetId::from_hash("modal-explicit-zero");
    h.frame(|ui| {
        Modal::new()
            .id(root_id)
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .min_size(Size::ZERO)
            .show(ui, |_, _| {});
    });

    let panel_id = root_id.with("panel");
    let panel = h.node_of(panel_id).expect("modal panel node");
    assert_eq!(panel.layer, Layer::Modal);
    let tree = h.ui.tree(Layer::Modal);
    assert_eq!(
        tree.records.layout()[panel.node.idx()].padding,
        Spacing::ZERO
    );
    assert_eq!(tree.bounds(panel.node).min_size, Size::ZERO);
}

/// A modal takes no placement, so the layer default makes the backdrop cover the screen; the root paints the scrim.
#[test]
fn the_backdrop_root_covers_the_whole_surface_in_the_scrim() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let id = WidgetId::from_hash("modal-full-surface");
    let custom = RgbaF32::srgba(0.2, 0.4, 0.6, 0.5);
    for explicit in [None, Some(custom)] {
        let mut h = UiHarness::new(SURFACE);
        let themed = h.ui.theme().modal.backdrop;
        h.frame(|ui| {
            let modal = Modal::new().id(id);
            match explicit {
                Some(c) => modal.backdrop(c),
                None => modal,
            }
            .show(ui, |_, _| {});
        });

        let backdrop = h.node_of(id).expect("modal backdrop recorded");
        assert_eq!(backdrop.layer, Layer::Modal);
        assert_eq!(
            h.ui.layout(Layer::Modal).rect[backdrop.node.idx()],
            Rect::new(0.0, 0.0, SURFACE.x as f32, SURFACE.y as f32),
        );
        let scrim =
            h.ui.tree(Layer::Modal)
                .chrome(backdrop.node)
                .expect("the backdrop paints a scrim")
                .fill;
        let want = RgbaF16::from(explicit.unwrap_or(themed));
        assert!(
            matches!(scrim, ShapeBrush::Solid(fill) if fill == want),
            "explicit {explicit:?}: scrim {scrim:?}, want {want:?}",
        );
    }
}

/// A modal paints above every popup, so it must also hear Escape, which holds
/// only while its scope outranks the popup's. The no-popup control separates
/// "layer ordering works" from "Escape works".
#[test]
fn modal_hears_escape_even_while_a_popup_below_holds_keyboard_claim() {
    fn escape_dismisses(with_popup: bool) -> bool {
        const SURFACE: UVec2 = UVec2::new(400, 300);
        let scene = |ui: &mut Ui| {
            if with_popup {
                Popup::new(Anchor::at_point(Vec2::ZERO))
                    .id(WidgetId::from_hash("under-modal"))
                    .show(ui, |_ui, _handle| {});
            }
            Modal::new()
                .id(WidgetId::from_hash("modal-escape"))
                .show(ui, |_, _| {})
                .dismissed
        };

        // Two frames: the wake-gate parks a press whose shortcut nobody watched yet.
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            scene(ui);
        });
        h.key(Key::Escape);
        h.frame_value(scene)
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

/// A dismissed modal hands both streams back on the next frame; a dismissing
/// frame's claim could otherwise swallow the click landing where it was.
/// This does not pin `Ui::release_input_scope` (a second record pass already
/// drops the claim; `release` is pinned in `input::tests::keyboard`).
#[test]
fn a_dismissed_modal_stops_owning_input_on_the_very_next_frame() {
    use crate::input::watch::{KeyboardWake, PointerWake};
    const SURFACE: UVec2 = UVec2::new(400, 300);

    let scene = |ui: &mut Ui, open: &mut bool| {
        ui.watch_pointer(PointerWake::BUTTONS);
        ui.watch_keyboard(KeyboardWake::KEY);
        if *open {
            *open = !Modal::new()
                .id(WidgetId::from_hash("closing-modal"))
                .show(ui, |_, _| {})
                .dismissed;
        }
    };

    let mut h = UiHarness::new(SURFACE);
    let mut open = true;
    h.frame(|ui| scene(ui, &mut open));

    h.key(Key::Escape);
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "Escape must dismiss the modal");

    h.press_at(Vec2::new(20.0, 20.0));
    h.key(Key::Char('a'));
    let [pointer, keyboard] = h.frame_value(|ui| {
        scene(ui, &mut open);
        [ui.pointer_events().len(), ui.keyboard_events().len()]
    });
    assert_eq!(pointer, 1, "the dismissed modal still held the pointer");
    assert_eq!(keyboard, 1, "the dismissed modal still held the keyboard");
}

/// Exactly one overlay may act on a given Escape: ownership resolves topmost-first.
#[test]
fn escape_closes_only_the_topmost_overlay() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    #[derive(Debug)]
    struct Closed {
        popup: bool,
        modal: bool,
    }
    let scene = |ui: &mut Ui| Closed {
        popup: Popup::new(Anchor::at_point(Vec2::ZERO))
            .id(WidgetId::from_hash("under-modal"))
            .show(ui, |_ui, _handle| {})
            .dismissed,
        modal: Modal::new()
            .id(WidgetId::from_hash("over-popup"))
            .show(ui, |_, _| {})
            .dismissed,
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        scene(ui);
    });
    h.key(Key::Escape);
    let closed = h.frame_value(scene);

    assert!(closed.modal, "the topmost overlay must take the Escape");
    assert!(
        !closed.popup,
        "the popup beneath the modal must not also consume it",
    );
}

/// The stock `modal.min_width` (280) is a default, so an authored `max_size` below it wins.
#[test]
fn an_authored_max_below_the_themed_min_width_wins() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root_id = WidgetId::from_hash("narrow-modal");
    assert!(
        h.ui.theme().modal.min_width > 240.0,
        "fixture: the stock floor is above 240"
    );
    h.prime(2, |ui| {
        Modal::new()
            .id(root_id)
            .max_size((240.0, 400.0))
            .show(ui, |_, _| {});
    });
    let panel = h.arranged(root_id.with("panel"));
    assert_eq!(panel.size.w, 240.0);
}
