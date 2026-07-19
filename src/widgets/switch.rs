use crate::forest::element::{Configure, Element, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::noop_f32;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::TextInput;
use crate::ui::Ui;
use crate::widgets::text::Text;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::{Response, WidgetEntry, enter_widget};
use glam::Vec2;

/// Track width as a multiple of its height. A switch reads as a switch
/// (not a checkbox) at roughly 7:4.
const TRACK_ASPECT: f32 = 1.75;

/// Two-state boolean toggle drawn as a pill track with a knob that
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
    element: Element,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Switch<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut element = Element::hstack();
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Borrow a theme override for this switch. The default inherits
    /// [`crate::Theme::switch`].
    pub fn style(mut self, s: &'a ToggleTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &self.element);
        if state.left.clicked() && !state.disabled {
            *self.value = !*self.value;
        }
        let on = *self.value;
        let label = self.label;

        // Resolve everything off the theme before the `&mut ui` animate
        // reborrow (the borrow may point into `ui.theme`).
        let theme = self.style.unwrap_or(&ui.theme.switch);
        let look_target = theme.pick(state, on).clone();
        let anim = theme.anim;
        let track_h = theme.box_size;
        let inset = theme.indicator_inset;
        let knob_color = theme.indicator;
        let row_gap = theme.row_gap;

        let fallback_text = ui.theme.text;
        let mut look = look_target.animate(ui, id, fallback_text, anim);
        look.background.corners = Corners::all(track_h * 0.5); // pill track

        // The track's stroke auto-insets the Canvas content box by its
        // width on every side (`Tree::open_node`), so the knob's declared
        // position is content-box-relative. Feed the stroke into
        // `switch_geom` so it subtracts it back out and the knob's margins
        // stay measured from the pill's outer edge — otherwise the knob
        // arranges a stroke-width low and to the right of centre.
        let stroke = look.background.stroke.width;
        let stroke_inset = if noop_f32(stroke) { 0.0 } else { stroke };
        let geom = switch_geom(track_h, inset, stroke_inset);

        let knob_id = id.with("knob");
        let target_x = if on { geom.on_x } else { geom.off_x };
        let knob_x = ui.animate(knob_id, "x", target_x, anim);
        let knob_bg = Background::rounded(knob_color, Corners::all(geom.knob * 0.5));

        let mut element = self.element;
        element.gaps.set_gap(row_gap);
        element.child_align = Align::v(VAlign::Center);
        ui.node(id, element, None, |ui| {
            let track_id = id.with("track");
            let mut track = Element::canvas();
            track.salt = Salt::Verbatim(track_id);
            track.size = (Sizing::fixed(geom.track_w), Sizing::fixed(track_h)).into();
            ui.node(track_id, track, Some(&look.background), |ui| {
                let mut knob = Element::leaf();
                knob.salt = Salt::Verbatim(knob_id);
                knob.size = (Sizing::fixed(geom.knob), Sizing::fixed(geom.knob)).into();
                knob.position = Vec2::new(knob_x, geom.knob_y);
                ui.node(knob_id, knob, Some(&knob_bg), |_| {});
            });

            if !label.is_empty() {
                Text::new(label)
                    .id(id.with("label"))
                    .style(look.text)
                    .text_align(Align::v(VAlign::Center))
                    .show(ui);
            }
        });

        Response::eager(id, ui, raw_state)
    }
}

impl Configure for Switch<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

#[derive(Debug)]
struct SwitchGeom {
    track_w: f32,
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
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
fn switch_geom(track_h: f32, inset: f32, stroke: f32) -> SwitchGeom {
    let track_w = track_h * TRACK_ASPECT;
    let knob = (track_h - 2.0 * inset).max(2.0);
    SwitchGeom {
        track_w,
        knob,
        off_x: inset - stroke,
        on_x: track_w - knob - inset - stroke,
        knob_y: inset - stroke,
    }
}

#[cfg(test)]
mod tests {
    use crate::Ui;
    use crate::forest::layer::Layer;
    use crate::widgets::switch::{Switch, switch_geom};
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
        let g = switch_geom(track_h, inset, stroke);
        assert!((g.track_w - 35.0).abs() < 1e-6);
        assert!((g.knob - 14.0).abs() < 1e-6);
        assert!((g.off_x - 2.0).abs() < 1e-6);
        assert!((g.on_x - 17.0).abs() < 1e-6);
        assert!((g.knob_y - 2.0).abs() < 1e-6);

        // Rect-relative margins (re-add the stroke the content box ate):
        // every one equals `inset`.
        let margins = [
            ("off left", stroke + g.off_x),
            ("on right", g.track_w - (stroke + g.on_x + g.knob)),
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
        let g = switch_geom(20.0, 3.0, 0.0);
        assert!((g.off_x - 3.0).abs() < 1e-6);
        assert!((g.on_x - 18.0).abs() < 1e-6);
        assert!((g.knob_y - 3.0).abs() < 1e-6);
    }

    /// A degenerate height can't drive the knob negative — it floors at
    /// 2 px.
    #[test]
    fn switch_geom_knob_floors_at_two() {
        let g = switch_geom(4.0, 3.0, 0.0); // 4 - 6 = -2 → floored
        assert!((g.knob - 2.0).abs() < 1e-6);
    }

    /// Regression: the off-state knob is centred in the track despite the
    /// track's 1 px stroke auto-insetting the Canvas content box. Before
    /// the stroke compensation the knob arranged at (4, 4) — 1 px low and
    /// 1 px right — leaving a 4/2 px top/bottom gap. It must rest `inset`
    /// (3 px) from every edge: offset (3, 3), 18 px of travel to the right.
    #[test]
    fn off_knob_is_centred_in_track() {
        let mut ui = Ui::for_test();
        let mut on = false;
        let root = ui.under_outer(UVec2::new(400, 400), |ui| {
            Switch::new(&mut on).label("Wi-Fi").show(ui).node()
        });
        let tree = &ui.forest.trees[Layer::Main];
        let track = tree.children(root).next().unwrap().id;
        let knob = tree.children(track).next().unwrap().id;
        let tr = ui.layout[Layer::Main].rect[track.idx()];
        let kr = ui.layout[Layer::Main].rect[knob.idx()];
        let left = kr.min.x - tr.min.x;
        let top = kr.min.y - tr.min.y;
        let right = (tr.min.x + tr.size.w) - (kr.min.x + kr.size.w);
        let bottom = (tr.min.y + tr.size.h) - (kr.min.y + kr.size.h);
        assert_eq!((left, top), (3.0, 3.0), "knob top-left margin");
        assert_eq!(top, bottom, "knob vertically centred");
        assert_eq!(right, 18.0, "off knob rests left with 18 px of travel");
    }
}
