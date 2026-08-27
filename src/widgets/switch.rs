use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::noop_f32;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::text_input::TextInput;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::widget_look::look_plan::LookPlan;
use crate::widgets::toggle_chrome::ToggleChrome;
use glam::Vec2;

/// Two-response boolean toggle drawn as a pill track with a knob that
/// slides between the ends — the iOS/Material "switch". Takes a
/// `&mut bool` whose owner controls the value; clicking the row flips
/// it. Visuals come from `theme.switch` ([`crate::ToggleTheme`]), which
/// defaults to an animated knob slide + track color cross-fade.
///
/// Layout mirrors [`crate::Checkbox`]: `HStack [track, label]`, one
/// `Sense::CLICK` hit target. The track is a `Canvas` so the knob can be
/// absolutely positioned; the knob's x animates through [`Ui::animate`].
#[derive(Debug)]
pub struct Switch<'a> {
    node: Node,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Switch<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    label_setter!('a, "Drawn to the right of the track; an empty label leaves the track alone.");

    style_setter!('a, ToggleTheme, switch);

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        if response.left.clicked() && !response.disabled {
            *self.value = !*self.value;
        }
        let on = *self.value;

        let theme = ui.theme();
        let slot = self.slot(theme);
        let track_h = slot.box_size;
        let inset = slot.indicator_inset;
        let aspect = slot.track_aspect;
        let knob_color = slot.indicator;
        let anim = slot.anim;
        let row_gap = slot.row_gap;
        let look = LookPlan {
            target: slot.pick(&response, on).to_animated(&theme.text),
            padding: slot.padding,
            margin: slot.margin,
            anim: slot.anim,
        }
        .apply(ui, &mut widget);

        let knob_id = id.with("knob");
        let chrome = ToggleChrome {
            look,
            row_gap,
            // A `Canvas` so the knob can be absolutely positioned inside
            // the track. Width is stroke-independent, so it resolves
            // here even though the stroke isn't known until the body.
            boxed: Node::canvas().size((
                Sizing::fixed(track_width(track_h, aspect)),
                Sizing::fixed(track_h),
            )),
            pill: Some(track_h * 0.5),
        };
        chrome.record_row(ui, widget, response, self.label, |ui, track| {
            // The track's stroke auto-insets the Canvas content box by
            // its width on every side (`Tree::open_node`), so the knob's
            // declared position is content-box-relative. Feed the stroke
            // into `switch_geom` so it subtracts it back out and the
            // knob's margins stay measured from the pill's outer edge —
            // otherwise the knob arranges a stroke-width low and to the
            // right of centre. Read off the *resolved* chrome, not the
            // theme: the stroke animates between the on and off looks,
            // and a mid-transition knob has to track it.
            let stroke = track.stroke.width;
            let stroke_inset = if noop_f32(stroke) { 0.0 } else { stroke };
            let geom = switch_geom(track_h, inset, stroke_inset, aspect);

            let target_x = if on { geom.on_x } else { geom.off_x };
            let knob_x = ui.animate(knob_id, "x", target_x, anim);
            let knob_bg = Background::rounded(knob_color, Corners::all(geom.knob * 0.5));
            let knob = Node::leaf()
                .id(knob_id)
                .size((Sizing::fixed(geom.knob), Sizing::fixed(geom.knob)))
                .position(Vec2::new(knob_x, geom.knob_y));
            ui.widget(knob).record(ui, Some(&knob_bg), |_| {});
        })
    }
}

impl_configure!(Switch<'_>);

/// Knob placement inside the track. The track's own extent is
/// [`track_width`] × `track_h` and is not repeated here — `Switch::show`
/// needs it one step earlier, before the chrome (and so the stroke)
/// resolves.
#[derive(Debug)]
struct SwitchGeom {
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
}

/// Track width for a `track_h`-tall switch. Split out because it does
/// not depend on the stroke: `Switch::show` sizes the track node from it
/// before the chrome resolves, while [`switch_geom`] needs the stroke to
/// place the knob.
fn track_width(track_h: f32, aspect: f32) -> f32 {
    track_h * aspect
}

/// Derive the track/knob geometry from the track height, knob inset, and
/// the track's `stroke` width. The knob is `track_h - 2*inset` (floored
/// at 2 px so a degenerate height can't invert it) and, measured from the
/// pill's outer edge, rests `inset` from the top and from whichever end
/// it sits against.
///
/// Returned x/y are **content-box-relative**: the track's stroke
/// auto-insets the Canvas content box by `stroke` on every side
/// (`Tree::open_node`), so each coordinate has `stroke` subtracted to land
/// the knob back at its intended rect-relative margin. Pass `stroke = 0`
/// for a borderless track and the coordinates are the plain rect insets.
fn switch_geom(track_h: f32, inset: f32, stroke: f32, aspect: f32) -> SwitchGeom {
    let track_w = track_width(track_h, aspect);
    let knob = (track_h - 2.0 * inset).max(2.0);
    SwitchGeom {
        knob,
        off_x: inset - stroke,
        on_x: track_w - knob - inset - stroke,
        knob_y: inset - stroke,
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::harness::UiHarness;

    use crate::scene::layer::Layer;
    use crate::widgets::switch::{Switch, switch_geom, track_width};

    /// The aspect `ToggleTheme::switch` ships; the expected numbers
    /// below are computed from it.
    const ASPECT: f32 = 1.75;
    use glam::UVec2;

    /// Geometry math for the 20 px default with a 1 px track stroke:
    /// `track_w = 35`, `knob = 14`. The stroke auto-insets the Canvas
    /// content box by 1 px on every side (`Tree::open_node`), so the
    /// returned content-box coords are `off_x = 2`, `on_x = 17`,
    /// `knob_y = 2`. Re-adding the stroke inset puts the knob exactly
    /// `inset` (3 px) from every rect edge in both rest states — i.e.
    /// vertically centred and horizontally symmetric.
    #[test]
    fn switch_geom_default_dimensions() {
        let (track_h, inset, stroke) = (20.0_f32, 3.0_f32, 1.0_f32);
        let track_w = track_width(track_h, ASPECT);
        let g = switch_geom(track_h, inset, stroke, ASPECT);
        assert!((track_w - 35.0).abs() < 1e-6);
        assert!((g.knob - 14.0).abs() < 1e-6);
        assert!((g.off_x - 2.0).abs() < 1e-6);
        assert!((g.on_x - 17.0).abs() < 1e-6);
        assert!((g.knob_y - 2.0).abs() < 1e-6);

        // Rect-relative margins (re-add the stroke the content box ate):
        // every one equals `inset`.
        let margins = [
            ("off left", stroke + g.off_x),
            ("on right", track_w - (stroke + g.on_x + g.knob)),
            ("top", stroke + g.knob_y),
            ("bottom", track_h - (stroke + g.knob_y + g.knob)),
        ];
        for (name, m) in margins {
            assert!(
                (m - inset).abs() < 1e-6,
                "{name} margin = {m}, want {inset}"
            );
        }
    }

    /// With no track stroke the content box equals the rect, so the
    /// coordinates degenerate to the plain rect insets: `off_x = inset`,
    /// `on_x = track_w - knob - inset`, `knob_y = inset`. Pinning this
    /// against `switch_geom_default_dimensions` shows the `stroke`
    /// argument actually moves the coordinates (off_x: 3 → 2).
    #[test]
    fn switch_geom_no_stroke_is_rect_relative() {
        let g = switch_geom(20.0, 3.0, 0.0, ASPECT);
        assert!((g.off_x - 3.0).abs() < 1e-6);
        assert!((g.on_x - 18.0).abs() < 1e-6);
        assert!((g.knob_y - 3.0).abs() < 1e-6);
    }

    /// A wider aspect stretches the track and pushes the on-response
    /// knob further right, while leaving the knob itself (a function of
    /// height alone) untouched. 20 px tall at 3:1 is a 60 px track, so
    /// the knob rests at `60 - 14 - 3 = 43` instead of `35 - 14 - 3 = 18`.
    #[test]
    fn track_aspect_stretches_the_track_not_the_knob() {
        let wide = switch_geom(20.0, 3.0, 0.0, 3.0);
        let stock = switch_geom(20.0, 3.0, 0.0, ASPECT);
        assert!((track_width(20.0, 3.0) - 60.0).abs() < 1e-6);
        assert!((wide.on_x - 43.0).abs() < 1e-6);
        assert!((stock.on_x - 18.0).abs() < 1e-6);
        assert_ne!(wide.on_x, stock.on_x);
        assert!((wide.knob - stock.knob).abs() < 1e-6);
    }

    /// A degenerate height can't drive the knob negative — it floors at
    /// 2 px.
    #[test]
    fn switch_geom_knob_floors_at_two() {
        let g = switch_geom(4.0, 3.0, 0.0, ASPECT); // 4 - 6 = -2 → floored
        assert!((g.knob - 2.0).abs() < 1e-6);
    }

    /// Regression: the off-response knob is centred in the track despite the
    /// track's 1 px stroke auto-insetting the Canvas content box. Before
    /// the stroke compensation the knob arranged at (4, 4) — 1 px low and
    /// 1 px right — leaving a 4/2 px top/bottom gap. It must rest `inset`
    /// (3 px) from every edge: offset (3, 3), 18 px of travel to the right.
    #[test]
    fn off_knob_is_centred_in_track() {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let mut on = false;
        let root = h.under_outer(|ui| Switch::new(&mut on).label("Wi-Fi").show(ui).node());
        let tree = h.ui.tree(Layer::Main);
        let track = tree.children(root).next().unwrap().id;
        let knob = tree.children(track).next().unwrap().id;
        let tr = h.ui.arranged_rect(Layer::Main, track);
        let kr = h.ui.arranged_rect(Layer::Main, knob);
        let left = kr.min.x - tr.min.x;
        let top = kr.min.y - tr.min.y;
        let right = (tr.min.x + tr.size.w) - (kr.min.x + kr.size.w);
        let bottom = (tr.min.y + tr.size.h) - (kr.min.y + kr.size.h);
        assert_eq!((left, top), (3.0, 3.0), "knob top-left margin");
        assert_eq!(top, bottom, "knob vertically centred");
        assert_eq!(right, 18.0, "off knob rests left with 18 px of travel");
    }
}
