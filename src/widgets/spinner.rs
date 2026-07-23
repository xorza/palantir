use crate::layout::types::sizing::Sizing;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::color::Color;
use crate::scene::element::{Configure, ConfigureElement, Element};
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::Ui;
use crate::widgets::Response;
use glam::Vec2;
use std::f32::consts::PI;
use std::time::Duration;

/// Arc length in radians — a 3/4 sweep leaves a visible gap so the
/// rotation is legible.
const SWEEP: f32 = 1.5 * PI;
/// Angular velocity (radians / second).
const SPEED: f32 = 4.5;

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
pub struct Spinner {
    element: Element,
    diameter: Option<f32>,
    color: Option<Color>,
    thickness: Option<f32>,
}

impl Spinner {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::leaf(),
            diameter: None,
            color: None,
            thickness: None,
        }
    }

    /// Diameter in logical px. `None` (default) inherits
    /// [`crate::Theme::spinner`].
    pub fn diameter(mut self, px: f32) -> Self {
        self.diameter = Some(px);
        self
    }

    /// Arc color (head of the comet). `None` (default) inherits
    /// [`crate::Theme::spinner`].
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// Stroke width in logical px. Default `diameter * 0.12` (min `1.5`).
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let theme = &ui.theme.spinner;
        let diameter = self.diameter.unwrap_or(theme.diameter).max(1.0);
        let width = self.thickness.unwrap_or((diameter * 0.12).max(1.5));
        let color = self.color.unwrap_or(theme.color);
        self.element
            .size
            .get_or_insert((Sizing::fixed(diameter), Sizing::fixed(diameter)).into());

        let widget = ui.widget(self.element);
        widget.node(ui, None, |ui| {
            // Static arc (phase 0) + a paint-time spin: the recorded
            // shape is identical every frame, so the spinner's subtree
            // stays cache-stable and only the composer re-spins it.
            let ArcGeometry { center, radius } = arc_geometry(diameter, width);
            ui.add_shape_animated(
                Shape::arc(center, radius, 0.0, SWEEP, width)
                    .brush(comet_brush(color))
                    .cap(LineCap::Round),
                PaintAnim::Spin {
                    speed: SPEED,
                    started_at: Duration::ZERO,
                },
            );
        });
        widget.response(ui)
    }
}

impl Configure for Spinner {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
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
fn comet_brush(base: Color) -> LinearGradient {
    LinearGradient::two_stop(0.0, base.with_alpha(0.0), base)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::color::{Color, ColorU8};
    use crate::scene::element::Configure;
    use crate::scene::layer::Layer;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::spinner::{ArcGeometry, SWEEP, arc_geometry, comet_brush};
    use glam::UVec2;
    use glam::Vec2;

    /// The trace circle insets by half the stroke width (round caps
    /// reach `width/2` past the centerline, so this keeps the painted
    /// stroke inside the box), and degenerate sizes clamp at zero.
    #[test]
    fn arc_geometry_insets_by_half_width() {
        assert_eq!(
            arc_geometry(24.0, 2.0),
            ArcGeometry {
                center: Vec2::splat(12.0),
                radius: 11.0,
            }
        );
        // width ≥ size: radius clamps to 0 instead of going negative.
        assert_eq!(arc_geometry(4.0, 8.0).radius, 0.0);
        // The recorded sweep leaves a visible gap (not a full circle).
        const { assert!(SWEEP < TAU) };
    }

    /// Comet trail: tail transparent, head the full color, rgb equal on
    /// both stops (only alpha fades). A translucent base scales — the
    /// head must carry the base alpha, not opaque 1.0.
    #[test]
    fn comet_brush_fades_tail_to_head() {
        let base = Color::rgb(0.6, 0.8, 1.0).with_alpha(0.5);
        let g = comet_brush(base);
        assert_eq!(g.stops.len(), 2);
        let tail = g.stops[0];
        let head = g.stops[1];
        assert_eq!(tail.offset(), 0.0);
        assert_eq!(head.offset(), 1.0);
        assert_eq!(tail.color.a, 0);
        assert_eq!(head.color, ColorU8::from(base));
        // RGB is untouched — only alpha varies along the trail.
        assert_eq!(tail.color.r, head.color.r);
        assert_eq!(tail.color.g, head.color.g);
        assert_eq!(tail.color.b, head.color.b);
    }

    #[test]
    fn explicit_layout_size_is_independent_from_diameter() {
        let mut ui = Ui::for_test();
        let (mut sized, mut hug, mut default) = (None, None, None);
        ui.run_at_without_baseline(UVec2::new(200, 120), |ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                sized = Some(
                    Spinner::new()
                        .diameter(12.0)
                        .size((Sizing::fixed(30.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .node(),
                );
                hug = Some(
                    Spinner::new()
                        .diameter(12.0)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui)
                        .node(),
                );
                default = Some(Spinner::new().diameter(12.0).show(ui).node());
            });
        });

        let rects = &ui.layout[Layer::Main].rect;
        let sized = rects[sized.unwrap().idx()];
        let hug = rects[hug.unwrap().idx()];
        let default = rects[default.unwrap().idx()];
        assert_eq!((sized.size.w, sized.size.h), (30.0, 40.0));
        assert_eq!((hug.size.w, hug.size.h), (0.0, 0.0));
        assert_eq!((default.size.w, default.size.h), (12.0, 12.0));
    }
}
