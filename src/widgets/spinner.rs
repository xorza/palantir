use crate::layout::types::sizing::Sizing;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::color::Color;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::spinner::SpinnerTheme;
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
    node: Node,
    diameter: Option<f32>,
    color: Option<Color>,
    thickness: Option<f32>,
    style: Option<&'a SpinnerTheme>,
}

impl<'a> Spinner<'a> {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            node: Node::leaf(),
            diameter: None,
            color: None,
            thickness: None,
            style: None,
        }
    }

    /// Borrow a theme override for this spinner. The default inherits
    /// [`crate::Theme::spinner`]. Per-field [`Self::color`] /
    /// [`Self::diameter`] / [`Self::thickness`] still win over it.
    pub fn style(mut self, s: &'a SpinnerTheme) -> Self {
        self.style = Some(s);
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
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// Stroke width in logical px, defaulting to the theme's
    /// diameter-derived width. One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().spinner);
        let diameter = self.diameter.unwrap_or(theme.diameter).max(1.0);
        let width = self
            .thickness
            .unwrap_or((diameter * theme.thickness_ratio).max(theme.min_thickness));
        let color = self.color.unwrap_or(theme.color);
        let sweep = theme.sweep;
        let speed = theme.speed;
        self.node
            .size
            .get_or_insert((Sizing::fixed(diameter), Sizing::fixed(diameter)).into());

        ui.widget(self.node)
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

impl_configure!(Spinner<'_>);

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
    use crate::ui::harness::UiHarness;
    use std::f32::consts::TAU;

    use crate::layout::types::sizing::Sizing;
    use crate::primitives::color::{Color, ColorU8};
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::spinner::{ArcGeometry, arc_geometry, comet_brush};
    use crate::widgets::theme::spinner::SpinnerTheme;
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
        // The default sweep leaves a visible gap — a full turn would
        // paint as a static ring, with nothing to read the spin off.
        assert!(SpinnerTheme::default().sweep < TAU);
    }

    /// Sweep, spin rate, and the diameter-derived stroke all come off
    /// `Theme::spinner` rather than constants. Stroke is
    /// `diameter * thickness_ratio` floored at `min_thickness`, so the
    /// arc keeps its proportions when the spinner is resized — and the
    /// floor is what a tiny one lands on.
    #[test]
    fn arc_and_spin_follow_the_spinner_theme() {
        use crate::scene::shapes::paint::CurveBasis;
        use crate::scene::shapes::record::ShapeRecord;
        use crate::scene::tree::paint_anims::PaintAnim;

        fn recorded(theme: SpinnerTheme, diameter: f32) -> (f32, f32, f32) {
            let mut h = UiHarness::new(UVec2::new(200, 200));
            h.ui.theme_mut().spinner = theme;
            h.frame(|ui| {
                Panel::hstack().auto_id().show(ui, |ui| {
                    Spinner::new()
                        .id(WidgetId::from_hash("spin"))
                        .diameter(diameter)
                        .show(ui);
                });
            });
            let tree = &h.ui.forest.trees[Layer::Main];
            let arc = tree
                .shapes
                .records
                .iter()
                .find_map(|s| match s {
                    ShapeRecord::Curve {
                        basis: CurveBasis::Arc { a1, .. },
                        width,
                        ..
                    } => Some((*a1, *width)),
                    _ => None,
                })
                .expect("spinner records one arc");
            let speed = tree
                .paint_anims
                .entries
                .iter()
                .find_map(|e| match e.anim {
                    PaintAnim::Spin { speed, .. } => Some(speed),
                    _ => None,
                })
                .expect("spinner registers a spin anim");
            (arc.0, arc.1, speed)
        }

        // Stock theme: stroke is the ratio applied to the diameter,
        // clear of the floor at 50 px.
        let stock = SpinnerTheme::default();
        let (sweep, width, speed) = recorded(stock.clone(), 50.0);
        assert!((sweep - stock.sweep).abs() < 1e-4, "sweep is themed");
        assert!((speed - stock.speed).abs() < 1e-4, "spin rate is themed");
        let expected = 50.0 * stock.thickness_ratio;
        assert!(
            (width - expected).abs() < 1e-4,
            "want {expected}, got {width}"
        );

        // Quarter the diameter and the stroke follows it down, rather
        // than staying put.
        let (_, small, _) = recorded(stock.clone(), 12.5);
        let expected_small = 12.5 * stock.thickness_ratio;
        assert!((small - expected_small).abs() < 1e-4);
        assert_ne!(width, small);

        // Below the floor the derived value loses.
        let tiny = stock.min_thickness / stock.thickness_ratio * 0.5;
        let (_, floored, _) = recorded(stock.clone(), tiny);
        assert!(
            (floored - stock.min_thickness).abs() < 1e-4,
            "tiny spinner floors at min_thickness, got {floored}",
        );

        // Retheme: every one of the three moves.
        let loud = SpinnerTheme {
            sweep: 1.0,
            speed: 9.0,
            thickness_ratio: 0.5,
            ..SpinnerTheme::default()
        };
        let (sweep_b, width_b, speed_b) = recorded(loud, 50.0);
        assert!((sweep_b - 1.0).abs() < 1e-4);
        assert!((speed_b - 9.0).abs() < 1e-4);
        assert!((width_b - 25.0).abs() < 1e-4);
        assert_ne!(sweep, sweep_b);
        assert_ne!(speed, speed_b);
        assert_ne!(width, width_b);
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
        let mut h = UiHarness::new(UVec2::new(200, 120));
        let (mut sized, mut hug, mut default) = (None, None, None);
        h.frame(|ui| {
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

        let rects = &h.ui.layout[Layer::Main].rect;
        let sized = rects[sized.unwrap().idx()];
        let hug = rects[hug.unwrap().idx()];
        let default = rects[default.unwrap().idx()];
        assert_eq!((sized.size.w, sized.size.h), (30.0, 40.0));
        assert_eq!((hug.size.w, hug.size.h), (0.0, 0.0));
        assert_eq!((default.size.w, default.size.h), (12.0, 12.0));
    }
}
