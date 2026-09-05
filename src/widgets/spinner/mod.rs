//! The indeterminate activity spinner: a rounded arc that rotates on the
//! paint clock, so an idle window animates it without recording.

use crate::layout::types::sizing::Sizing;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::color::RgbaF32;
use crate::primitives::num::F32Ext;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::theme::spinner::SpinnerTheme;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::time::Duration;

/// Indeterminate activity spinner: a rounded arc that rotates with the
/// frame clock, its tail fading to transparent (a "comet" trail). The
/// internal spin animation's every-frame wake keeps the host repainting
/// while the spinner is recorded — on the PaintOnly fast path, with no
/// record/layout per tick — and costs nothing when it isn't.
///
/// The recorded [`Shape::arc`] is **identical every frame** (phase 0),
/// so its `subtree_hash` is stable and measure/cascade skip the
/// spinner's subtree; the live rotation is a paint-time
/// spin animation sampled from the frame clock — the composer
/// shifts the arc's angles when it emits the GPU instances, no
/// geometry is rebuilt. The arc renders natively on the GPU (exact
/// circle, adaptive subdivision), so it stays smooth at any size and
/// DPI; the comet fade is a linear gradient sampled along the sweep.
#[derive(Debug)]
pub struct Spinner<'a> {
    widget: Widget,
    diameter: Option<f32>,
    color: Option<RgbaF32>,
    thickness: Option<f32>,
    style: Option<&'a SpinnerTheme>,
}

impl<'a> Spinner<'a> {
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::leaf(),
            diameter: None,
            color: None,
            thickness: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `spinner`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// Per-field [`Self::color`] / [`Self::diameter`] / [`Self::thickness`]
    /// still win over it.
    pub fn style(mut self, s: impl Into<Option<&'a SpinnerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Diameter in logical px, defaulting to
    /// [`crate::Theme::spinner`]'s. One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn diameter(mut self, px: f32) -> Self {
        self.diameter = Some(px);
        self
    }

    /// Arc color (head of the comet), defaulting to
    /// [`crate::Theme::spinner`]'s. One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn color(mut self, c: RgbaF32) -> Self {
        self.color = Some(c);
        self
    }

    /// Stroke width in logical px, defaulting to the theme's
    /// diameter-derived width. One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().spinner);
        let diameter = self.diameter.unwrap_or(theme.diameter).themed_length(1.0);
        let width = self
            .thickness
            .unwrap_or((diameter * theme.thickness_ratio).max(theme.min_thickness));
        let color = self.color.unwrap_or(theme.color);
        let sweep = theme.sweep;
        let speed = theme.speed;
        self.widget
            .default_size((Sizing::fixed(diameter), Sizing::fixed(diameter)))
            .show(ui, None, |ui| {
                // Static arc (phase 0) + a paint-time spin: the recorded
                // shape is identical every frame, so the spinner's subtree
                // stays cache-stable and only the composer re-spins it.
                let ArcGeometry { center, radius } = arc_geometry(diameter, width);
                ui.add_shape_animated(
                    Shape::arc(center, radius, 0.0, sweep, width)
                        .brush(comet_brush(color))
                        .cap(LineCap::Round),
                    PaintAnim::Spin {
                        speed,
                        started_at: Duration::ZERO,
                    },
                );
            })
            .response
    }
}

impl Configure for Spinner<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// Node-local circle the arc traces.
#[derive(Debug, PartialEq)]
struct ArcGeometry {
    center: Vec2,
    radius: f32,
}

/// Inset the trace circle by half the stroke width so the stroke (and
/// its round caps, which reach `width/2` past the centerline) stays
/// inside the widget box.
fn arc_geometry(diameter: f32, width: f32) -> ArcGeometry {
    ArcGeometry {
        center: Vec2::splat(diameter * 0.5),
        radius: (diameter - width).max(0.0) * 0.5,
    }
}

/// Comet-trail gradient along the sweep: fully transparent at the tail
/// (t = 0, the arc's start angle), the full color at the head (t = 1).
/// Scaling from the base alpha keeps a translucent base translucent.
/// The gradient's `angle` is ignored on stroke shapes — the arc
/// carries its own 1-D parameter.
fn comet_brush(base: RgbaF32) -> LinearGradient {
    LinearGradient::two_stop(0.0, base.with_alpha(0.0), base)
}

#[cfg(test)]
mod tests;
