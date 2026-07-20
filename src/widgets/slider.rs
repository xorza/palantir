use crate::forest::element::{Configure, Element, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::slider::SliderTheme;
use crate::widgets::{Response, enter_widget};
use std::ops::RangeInclusive;

/// Horizontal value slider over a `f32` range. Takes a `&mut f32`;
/// dragging (or clicking) the rail moves the value. The knob position is
/// derived from the value with the same two-`Fill`-leaf trick as
/// [`crate::ProgressBar`] — `Fill(fraction)` left of the knob,
/// `Fill(1 − fraction)` right — so it tracks the resolved width without
/// the widget knowing it at record time. Pointer→value mapping uses last
/// frame's arranged width (one-frame lag, invisible at interactive
/// rates). Visuals come from [`crate::SliderTheme`] (theme slot
/// `slider`).
#[derive(Debug)]
pub struct Slider<'a> {
    element: Element,
    value: &'a mut f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    style: Option<&'a SliderTheme>,
}

impl<'a> Slider<'a> {
    #[track_caller]
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        let mut element = Element::hstack();
        element.flags.set_sense(Sense::CLICK | Sense::DRAG);
        Self {
            element,
            value,
            min: *range.start(),
            max: *range.end(),
            step: None,
            style: None,
        }
    }

    /// Snap the value to multiples of `step` (anchored at `min`). `0` or
    /// negative disables snapping (the default — continuous).
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    /// Borrow a theme override for this slider. The default inherits
    /// [`crate::Theme::slider`].
    pub fn style(mut self, s: &'a SliderTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let entry = enter_widget(ui, &self.element);
        let id = entry.id;
        let state = &entry.state;

        let theme = self.style.unwrap_or(&ui.theme.slider);
        let knob = theme.knob_size;
        let rail_h = theme.rail_thickness;
        let fill_color = theme.fill;
        let rail_color = theme.rail;
        let knob_color = theme.knob;

        // Pointer drives the value: pressing or dragging the rail maps
        // the cursor x against the last frame's logical width.
        if !state.disabled
            && (state.pressed() || state.left.drag.dragging())
            && let (Some(local), Some(rect)) = (state.pointer_local, state.layout_rect)
        {
            let f = pointer_to_fraction(local.x, rect.size.w, knob);
            let v = snap_to_step(
                fraction_to_value(f, self.min, self.max),
                self.min,
                self.step,
            );
            *self.value = clamp_range(v, self.min, self.max);
        }
        let fraction = value_to_fraction(*self.value, self.min, self.max);

        let pill = Corners::all(rail_h * 0.5);
        let fill_bg = Background::rounded(fill_color, pill);
        let rail_bg = Background::rounded(rail_color, pill);
        let knob_bg = Background::rounded(knob_color, Corners::all(knob * 0.5));

        let mut element = self.element;
        // `Sizes::default()` (Hug×Hug) = "caller didn't set a size" —
        // the same sentinel convention as theme padding/margin.
        if element.size == Sizes::default() {
            element.size = (Sizing::FILL, Sizing::fixed(knob)).into();
        }
        element.child_align = Align::v(VAlign::Center);

        ui.node(id, element, None, |ui| {
            rail_leaf(
                ui,
                id.with("fill"),
                Sizing::share(fraction),
                rail_h,
                &fill_bg,
            );
            knob_leaf(ui, id.with("knob"), knob, &knob_bg);
            rail_leaf(
                ui,
                id.with("rail"),
                Sizing::share(1.0 - fraction),
                rail_h,
                &rail_bg,
            );
        });
        entry.into_response(ui)
    }
}

impl Configure for Slider<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

fn rail_leaf(ui: &mut Ui, id: WidgetId, w: Sizing, h: f32, bg: &Background) {
    let mut el = Element::leaf();
    el.salt = Salt::Verbatim(id);
    el.size = (w, Sizing::fixed(h)).into();
    ui.node(id, el, Some(bg), |_| {});
}

fn knob_leaf(ui: &mut Ui, id: WidgetId, size: f32, bg: &Background) {
    let mut el = Element::leaf();
    el.salt = Salt::Verbatim(id);
    el.size = (Sizing::fixed(size), Sizing::fixed(size)).into();
    ui.node(id, el, Some(bg), |_| {});
}

/// Fraction (0..1) of the way from `min` to `max` that `value` sits.
/// Degenerate (`min == max`) ranges map to 0.
fn value_to_fraction(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if span.abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

/// Inverse of [`value_to_fraction`]: the value at `fraction` of the
/// range.
fn fraction_to_value(fraction: f32, min: f32, max: f32) -> f32 {
    min + fraction.clamp(0.0, 1.0) * (max - min)
}

/// Map a cursor x (relative to the rail's left edge) to a fraction. The
/// usable travel is `[knob/2, track_w - knob/2]` so the knob center
/// stays inside the rail at both extremes.
fn pointer_to_fraction(local_x: f32, track_w: f32, knob: f32) -> f32 {
    let travel = (track_w - knob).max(1.0);
    ((local_x - knob * 0.5) / travel).clamp(0.0, 1.0)
}

/// Snap to the nearest multiple of `step` measured from `min`. A `None`
/// or non-positive step is a passthrough.
fn snap_to_step(value: f32, min: f32, step: Option<f32>) -> f32 {
    match step {
        Some(s) if s > 0.0 => min + ((value - min) / s).round() * s,
        _ => value,
    }
}

/// Clamp into `[min, max]` tolerating a reversed pair.
fn clamp_range(value: f32, min: f32, max: f32) -> f32 {
    value.clamp(min.min(max), min.max(max))
}

#[cfg(test)]
mod tests {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::layer::Layer;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::transform::TranslateScale;
    use crate::primitives::widget_id::WidgetId;
    use crate::widgets::panel::Panel;
    use crate::widgets::slider::{
        Slider, clamp_range, fraction_to_value, pointer_to_fraction, snap_to_step,
        value_to_fraction,
    };
    use glam::{UVec2, Vec2};

    /// Explicit `.size(...)` wins over the widget's `Fill × knob_size`
    /// default (the `Sizes::default()` "caller didn't set a size"
    /// sentinel), and an untouched slider still gets that default
    /// (400-wide FILL column → 400 × knob_size 18) — the sentinel
    /// changes behavior in both directions.
    #[test]
    fn explicit_size_overrides_fill_default() {
        let mut ui = Ui::for_test();
        let mut v = 0.5_f32;
        let (mut sized, mut default) = (None, None);
        ui.run_at(UVec2::new(400, 300), |ui| {
            let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
            col.show(ui, |ui| {
                sized = Some(
                    Slider::new(&mut v, 0.0..=1.0)
                        .size((Sizing::fixed(120.0), Sizing::fixed(30.0)))
                        .show(ui)
                        .node(),
                );
                default = Some(Slider::new(&mut v, 0.0..=1.0).show(ui).node());
            });
        });
        let rects = &ui.layout[Layer::Main].rect;
        let s = rects[sized.unwrap().idx()];
        assert_eq!((s.size.w, s.size.h), (120.0, 30.0), "explicit size");
        let d = rects[default.unwrap().idx()];
        assert_eq!((d.size.w, d.size.h), (400.0, 18.0), "untouched default");
    }

    #[test]
    fn endpoint_rails_collapse_without_invalid_fill_weights() {
        for (value, expected) in [(0.0, [0.0, 18.0, 102.0]), (1.0, [102.0, 18.0, 0.0])] {
            let mut ui = Ui::for_test();
            let mut value = value;
            let root = ui.run_at_value(UVec2::new(120, 30), |ui| {
                Slider::new(&mut value, 0.0..=1.0)
                    .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                    .show(ui)
                    .node()
            });
            let widths: Vec<_> = ui
                .main_child_rects(root)
                .into_iter()
                .map(|rect| rect.size.w)
                .collect();
            assert_eq!(widths, expected, "value {value}");
        }
    }

    #[test]
    fn value_to_fraction_maps_and_clamps() {
        let cases = [
            (50.0, 0.0, 100.0, 0.5),
            (0.0, 0.0, 100.0, 0.0),
            (100.0, 0.0, 100.0, 1.0),
            (150.0, 0.0, 100.0, 1.0), // above clamps
            (-10.0, 0.0, 100.0, 0.0), // below clamps
            (15.0, 10.0, 20.0, 0.5),  // offset range
            (5.0, 3.0, 3.0, 0.0),     // degenerate
        ];
        for (v, min, max, want) in cases {
            let got = value_to_fraction(v, min, max);
            assert!(
                (got - want).abs() < 1e-6,
                "v2f({v},{min},{max})={got} want {want}"
            );
        }
    }

    #[test]
    fn fraction_to_value_inverts_value_to_fraction() {
        // Round-trip over an offset range.
        for &v in &[10.0_f32, 12.5, 15.0, 17.5, 20.0] {
            let f = value_to_fraction(v, 10.0, 20.0);
            let back = fraction_to_value(f, 10.0, 20.0);
            assert!((back - v).abs() < 1e-5, "roundtrip {v} -> {f} -> {back}");
        }
        assert!((fraction_to_value(0.25, 10.0, 20.0) - 12.5).abs() < 1e-6);
        // Out-of-range fraction clamps before mapping.
        assert!((fraction_to_value(1.5, 0.0, 100.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn pointer_to_fraction_uses_knob_inset_travel() {
        let track_w = 120.0;
        let knob = 20.0; // travel = 100, offset knob/2 = 10
        assert!((pointer_to_fraction(10.0, track_w, knob) - 0.0).abs() < 1e-6);
        assert!((pointer_to_fraction(110.0, track_w, knob) - 1.0).abs() < 1e-6);
        assert!((pointer_to_fraction(60.0, track_w, knob) - 0.5).abs() < 1e-6);
        // Past the ends clamps.
        assert!((pointer_to_fraction(0.0, track_w, knob) - 0.0).abs() < 1e-6);
        assert!((pointer_to_fraction(200.0, track_w, knob) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pointer_mapping_is_scale_invariant() {
        let id = WidgetId::from_hash("scaled-slider");
        for scale in [0.5, 1.0, 2.0] {
            for (local_x, expected) in [(9.0, 0.0), (34.5, 0.25), (111.0, 1.0)] {
                let mut ui = Ui::for_test();
                let mut value = 0.5;
                let build = |ui: &mut Ui, value: &mut f32| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("scaled-slider-parent"))
                        .transform(TranslateScale::from_scale(scale))
                        .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                        .show(ui, |ui| {
                            Slider::new(value, 0.0..=1.0)
                                .id(id)
                                .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                                .show(ui);
                        });
                };
                ui.run_at_acked(UVec2::new(300, 100), |ui| build(ui, &mut value));

                let response = ui.response_for(id);
                let layout = response.layout_rect.expect("slider arranged");
                let pointer = response
                    .transform
                    .apply_point(layout.min + Vec2::new(local_x, 9.0));
                ui.press_at(pointer);
                ui.run_at_acked(UVec2::new(300, 100), |ui| build(ui, &mut value));

                assert!(
                    (value - expected).abs() < 1e-6,
                    "logical x={local_x} at {scale}× produced {value}, expected {expected}",
                );
            }
        }
    }

    #[test]
    fn snap_to_step_rounds_to_grid() {
        assert!((snap_to_step(53.0, 0.0, Some(10.0)) - 50.0).abs() < 1e-6);
        assert!((snap_to_step(57.0, 0.0, Some(10.0)) - 60.0).abs() < 1e-6);
        assert!((snap_to_step(12.0, 0.0, Some(5.0)) - 10.0).abs() < 1e-6);
        assert!((snap_to_step(13.0, 0.0, Some(5.0)) - 15.0).abs() < 1e-6);
        // Off-anchor grid: steps of 0.5 from min=1.0.
        assert!((snap_to_step(2.2, 1.0, Some(0.5)) - 2.0).abs() < 1e-6);
        // None / non-positive passes through.
        assert!((snap_to_step(53.0, 0.0, None) - 53.0).abs() < 1e-6);
        assert!((snap_to_step(53.0, 0.0, Some(0.0)) - 53.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_range_tolerates_reversed_bounds() {
        assert!((clamp_range(5.0, 0.0, 10.0) - 5.0).abs() < 1e-6);
        assert!((clamp_range(-1.0, 0.0, 10.0) - 0.0).abs() < 1e-6);
        assert!((clamp_range(11.0, 0.0, 10.0) - 10.0).abs() < 1e-6);
        // Reversed pair clamps the same.
        assert!((clamp_range(11.0, 10.0, 0.0) - 10.0).abs() < 1e-6);
    }
}
