//! The `Ui` a test drives, and the frames it records into.

use crate::animation::animation_spec::AnimationSpec;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::tree::node_id::NodeId;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::Ui;
use crate::ui::resources::UiResources;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::UVec2;
use std::time::Duration;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 200);

pub(super) fn ui_with_shared(shared: &UiResources) -> Harness {
    Harness::from_resources(shared.clone(), SURFACE)
}

pub(super) fn blue_frame(ui: &mut Ui, salt: &'static str) -> NodeId {
    Block::new()
        .id(WidgetId::from_hash(salt))
        .size(50.0)
        .chrome(Chrome::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
        .show(ui)
        .node(ui)
}

pub(super) fn add_blink_shape(ui: &mut Ui, half: Duration) {
    use crate::shape::Shape;
    use crate::shape::paint_animation::PaintAnimation;
    use crate::shape::paint_animation::PaintRepeat;
    use crate::shape::paint_animation::curves;

    ui.add_shape_animated(
        Shape::rect(RgbaF32::srgb(1.0, 0.0, 0.0)).at(Rect::new(0.0, 0.0, 4.0, 12.0)),
        PaintAnimation::alpha(0.0, 1.0)
            .with_started_at(Duration::ZERO)
            .with_period(half * 2)
            .with_steps(2)
            .with_repeat(PaintRepeat::Settle(Duration::MAX))
            .with_curve(curves::square),
    );
}

/// A harness whose first frame runs the warmup pass, on [`SURFACE`].
pub(super) fn cold_ui() -> Harness {
    Harness::cold(SURFACE)
}

/// `scheme`'s stock theme, which any change to it lands on at once.
pub(super) fn still_theme(scheme: ColorScheme) -> Theme {
    Theme {
        transition: AnimationSpec::SNAP,
        ..Theme::stock(scheme)
    }
}

/// Give a window that has shown nothing yet [`still_theme`] for both schemes.
pub(super) fn set_still_themes(h: &mut Harness) {
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        h.ui_mut().set_theme(scheme, still_theme(scheme)).unwrap();
    }
}
