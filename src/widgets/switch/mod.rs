//! The pill-and-knob boolean toggle — the same contract as the checkbox,
//! drawn as a switch.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::approx;
use crate::primitives::math::num::F32Ext;
use crate::primitives::paint::background::Background;
use crate::primitives::text::text_input::TextInput;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::theme::toggle::ToggleTheme;
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
#[must_use = "a widget records nothing until `show`"]
pub struct Switch<'a> {
    widget: Widget,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Switch<'a> {
    /// A switch bound to `value`, which a click flips.
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        Self {
            widget: ToggleChrome::row(),
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    /// The text this widget draws. Empty (the default) draws none —
    /// no text child is recorded at all.
    ///
    /// Drawn to the right of the track; an empty label leaves the track
    /// alone.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `switch`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the row and hand back its [`Response`].
    ///
    /// **`clicked()` is the change edge**, for the reason
    /// [`Checkbox::show`](crate::Checkbox::show) gives: a switch flips on
    /// every activation, so a click always writes the bound `bool`.
    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        let on = ToggleChrome::toggled(&response, self.value);

        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.switch);
        let track_h = slot.box_size.themed_length(1.0);
        let inset = slot.indicator_inset.themed_length(0.0);
        let aspect = slot.track_aspect;
        let knob_color = slot.indicator;
        let anim = slot.defaults.anim;
        let knob_id = id.with("knob");
        let chrome = ToggleChrome {
            plan: slot.plan(&response, on, theme.text),
            gap: slot.gap,
            // A `Canvas` so the knob can be absolutely positioned inside
            // the track. Width is border-independent, so it resolves
            // here even though the border isn't known until the body.
            boxed: Widget::canvas().size((
                Sizing::fixed(track_width(track_h, aspect)),
                Sizing::fixed(track_h),
            )),
            pill: Some(track_h * 0.5),
        };
        chrome.record_row(ui, self.widget, response, self.label, |ui, track| {
            // The track's border auto-insets the Canvas content box by
            // its width on every side (`Tree::open_node`), so the knob's
            // declared position is content-box-relative. Feed the border
            // into `switch_geom` so it subtracts it back out and the
            // knob's margins stay measured from the pill's outer edge —
            // otherwise the knob arranges a border-width low and to the
            // right of centre. Read off the *resolved* chrome, not the
            // theme: the border animates between the on and off looks,
            // and a mid-transition knob has to track it.
            let border = track.border.width;
            let border_inset = if approx::paints_nothing(border) {
                0.0
            } else {
                border
            };
            let geom = switch_geom(track_h, inset, border_inset, aspect);

            let target_x = if on { geom.on_x } else { geom.off_x };
            let knob_x = ui.animate(knob_id, "x", target_x, anim);
            let knob_bg = Background::rounded(knob_color, Corners::all(geom.knob * 0.5));
            let knob = Widget::leaf()
                .id(knob_id)
                .size((Sizing::fixed(geom.knob), Sizing::fixed(geom.knob)))
                .position(Vec2::new(knob_x, geom.knob_y));
            knob.record(ui, Some(&knob_bg), |_| {});
        })
    }
}

impl Configure for Switch<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// Knob placement inside the track. The track's own extent is
/// [`track_width`] × `track_h` and is not repeated here — `Switch::show`
/// needs it one step earlier, before the chrome (and so the border)
/// resolves.
#[derive(Debug)]
struct SwitchGeom {
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
}

/// Track width for a `track_h`-tall switch. Split out because it does
/// not depend on the border: `Switch::show` sizes the track node from it
/// before the chrome resolves, while [`switch_geom`] needs the border to
/// place the knob.
fn track_width(track_h: f32, aspect: f32) -> f32 {
    track_h * aspect
}

/// Derive the track/knob geometry from the track height, knob inset, and
/// the track's `border` width. The knob is `track_h - 2*inset` (floored
/// at 2 px so a degenerate height can't invert it) and, measured from the
/// pill's outer edge, rests `inset` from the top and from whichever end
/// it sits against.
///
/// Returned x/y are **content-box-relative**: the track's border
/// auto-insets the Canvas content box by `border` on every side
/// (`Tree::open_node`), so each coordinate has `border` subtracted to land
/// the knob back at its intended rect-relative margin. Pass `border = 0`
/// for a borderless track and the coordinates are the plain rect insets.
fn switch_geom(track_h: f32, inset: f32, border: f32, aspect: f32) -> SwitchGeom {
    let track_w = track_width(track_h, aspect);
    let knob = (track_h - 2.0 * inset).max(2.0);
    SwitchGeom {
        knob,
        off_x: inset - border,
        on_x: track_w - knob - inset - border,
        knob_y: inset - border,
    }
}

#[cfg(test)]
mod tests;
