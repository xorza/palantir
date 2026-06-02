use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
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
pub struct ToggleSwitch<'a> {
    element: Element,
    value: &'a mut bool,
    label: InternedStr,
    style: Option<ToggleTheme>,
}

impl<'a> ToggleSwitch<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            value,
            label: InternedStr::default(),
            style: None,
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr>) -> Self {
        self.label = s.into();
        self
    }

    /// Override the theme for this switch. `None` (default) inherits
    /// [`crate::Theme::switch`].
    pub fn style(mut self, s: ToggleTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &self.element);
        if state.clicked && !state.disabled {
            *self.value = !*self.value;
        }
        let on = *self.value;

        // Resolve everything off the theme before the `&mut ui` animate
        // reborrow (the borrow may point into `ui.theme`).
        let theme = self.style.as_ref().unwrap_or(&ui.theme.switch);
        let look_target = theme.pick(state, on).clone();
        let anim = theme.anim;
        let track_h = theme.box_size;
        let inset = theme.indicator_inset;
        let knob_color = theme.indicator;
        let row_gap = theme.row_gap;
        let geom = switch_geom(track_h, inset);

        let fallback_text = ui.theme.text;
        let mut look = look_target.animate(ui, id, fallback_text, anim);
        look.background.corners = Corners::all(track_h * 0.5); // pill track

        let knob_id = id.with("knob");
        let target_x = if on { geom.on_x } else { geom.off_x };
        let knob_x = ui.animate(knob_id, "x", target_x, anim);
        let knob_bg = Background {
            corners: Corners::all(geom.knob * 0.5),
            ..Background::fill(knob_color)
        };

        let mut element = self.element;
        element.gaps.set_gap(row_gap);
        element.child_align = Align::v(VAlign::Center);
        let label = self.label;

        ui.node(id, element, None, |ui| {
            let track_id = id.with("track");
            let mut track = Element::new(LayoutMode::Canvas);
            track.salt = Salt::Verbatim(track_id);
            track.size = (Sizing::Fixed(geom.track_w), Sizing::Fixed(track_h)).into();
            ui.node(track_id, track, Some(&look.background), |ui| {
                let mut knob = Element::new(LayoutMode::Leaf);
                knob.salt = Salt::Verbatim(knob_id);
                knob.size = (Sizing::Fixed(geom.knob), Sizing::Fixed(geom.knob)).into();
                knob.position = Vec2::new(knob_x, inset);
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

impl Configure for ToggleSwitch<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

struct SwitchGeom {
    track_w: f32,
    knob: f32,
    off_x: f32,
    on_x: f32,
}

/// Derive the track/knob geometry from the track height and inset. The
/// knob is `track_h - 2*inset` (floored at 2 px so a degenerate height
/// can't invert it) and travels from `off_x = inset` to
/// `on_x = track_w - knob - inset`.
fn switch_geom(track_h: f32, inset: f32) -> SwitchGeom {
    let track_w = track_h * TRACK_ASPECT;
    let knob = (track_h - 2.0 * inset).max(2.0);
    SwitchGeom {
        track_w,
        knob,
        off_x: inset,
        on_x: track_w - knob - inset,
    }
}

#[cfg(test)]
mod tests {
    use super::switch_geom;

    /// Geometry math: knob diameter, both rest positions, and the
    /// symmetry of the off/on insets. Hand-computed for the 20 px
    /// default: track_w = 35, knob = 14, off = 3, on = 18.
    #[test]
    fn switch_geom_default_dimensions() {
        let g = switch_geom(20.0, 3.0);
        assert!((g.track_w - 35.0).abs() < 1e-6);
        assert!((g.knob - 14.0).abs() < 1e-6);
        assert!((g.off_x - 3.0).abs() < 1e-6);
        assert!((g.on_x - 18.0).abs() < 1e-6);
        // The knob sits the same distance from each end at rest.
        let right_gap = g.track_w - (g.on_x + g.knob);
        assert!(
            (g.off_x - right_gap).abs() < 1e-6,
            "off/on insets asymmetric"
        );
    }

    /// A degenerate height can't drive the knob negative — it floors at
    /// 2 px.
    #[test]
    fn switch_geom_knob_floors_at_two() {
        let g = switch_geom(4.0, 3.0); // 4 - 6 = -2 → floored
        assert!((g.knob - 2.0).abs() < 1e-6);
    }
}
