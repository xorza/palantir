//! Visual fixtures: UI scenes rendered headlessly and compared against golden PNGs.

mod blit;
mod clip;
mod color;
mod corners;
mod damage;
mod damage_oracle;
mod expander;
mod fade;
mod format_change;
mod gpu_view;
mod gradient;
mod hidpi;
mod icon;
mod image;
mod layout;
mod material;
mod occlusion;
mod scroll;
mod shadow;
mod shapes;
mod tabs;
mod text;
mod turn;
mod user_scale;
mod widgets;

use std::fmt;

use palantir::{
    Configure, Image, PaintedBy, Panel, Rect, RectShape, RgbaF32, Shape, SizeSpec, Sizing, SrgbaU8,
    Ui,
};

use crate::visual_rig::VisualRig;

use crate::visual_rig::FIXTURE_PALETTE;

/// The scene background: the suite palette's window color, `VisualRig::clear`'s default. Set `RgbaF32::BLACK` there for harder contrast.
pub(crate) const DARK_BG: RgbaF32 = FIXTURE_PALETTE.window_bg;

/// Tolerance for a probed channel against its 8-bit sRGB value: linear `f32` through an `f16` tint into an 8-bit target errs only by the final rounding, within one step.
pub(crate) const SRGB_ROUND_TRIP: u8 = 1;

/// Every channel of `got` is within [`SRGB_ROUND_TRIP`] of `want`, or panic naming `what`.
#[track_caller]
pub(crate) fn assert_px(got: SrgbaU8, want: SrgbaU8, what: impl fmt::Display) {
    let worst = got.max_channel_diff(want);
    assert!(
        worst <= SRGB_ROUND_TRIP,
        "{what}: got {got:?}, want {want:?} — a channel is {worst} steps off, \
         {SRGB_ROUND_TRIP} allowed",
    );
}

/// `paint` over a rect filling a cell at `at`, told apart from its siblings by where it sits.
pub(crate) fn cell<P>(ui: &mut Ui, at: Rect, paint: P)
where
    RectShape: PaintedBy<P>,
{
    Panel::zstack()
        .id_salt(("cell", at.min.x.to_bits(), at.min.y.to_bits()))
        .at(at.min)
        .size((Sizing::fixed(at.size.w), Sizing::fixed(at.size.h)))
        .show(ui, |ui| {
            ui.add_shape(Shape::rect(), paint);
        });
}

/// `rig`'s frame of `scene` on a full-surface [`canvas`], read back.
pub(crate) fn render(rig: &mut VisualRig, mut scene: impl FnMut(&mut Ui)) -> Image {
    rig.frame(|ui| canvas(ui, &mut scene)).image
}

/// A canvas filling the surface, for content placed in absolute coordinates.
pub(crate) fn canvas(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::canvas()
        .id_salt("canvas")
        .size(SizeSpec::FILL)
        .show(ui, body);
}
