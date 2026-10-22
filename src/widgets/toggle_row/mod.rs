//! The shared `HStack [box, label]` scaffolding behind the toggle widgets.

use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::text::text_input::TextInput;
use crate::theme::look::look_plan::LookPlan;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;

/// One toggle row as [`ToggleRow::record_row`] records it, all but the box's body.
/// The theme arrives planned, not applied: only the caller knows which slot
/// (`theme.checkbox`/`radio`/`switch`) is its own.
#[derive(Debug)]
pub(crate) struct ToggleRow<'a> {
    /// The row, from [`row`].
    pub(crate) row: Widget,
    /// The row's state, read before the caller decided whether it toggled.
    pub(crate) response: ResponseState,
    /// The text right of the box; empty records no text child.
    pub(crate) label: TextInput<'a>,
    /// The look this row animates toward.
    pub(crate) plan: LookPlan,
    /// Gap between the box and the label.
    pub(crate) gap: f32,
    /// The box or track child, already sized; [`Self::record_row`] stamps its id and chrome onto it.
    pub(crate) boxed: Widget,
    /// Corner radius forced onto the box chrome so the radio pip and switch track read as pills; `None` keeps the theme's.
    pub(crate) pill: Option<f32>,
}

/// The row every toggle starts from: a click-sensing horizontal stack, as box and label are one hit target.
#[track_caller]
pub(crate) fn row() -> Widget {
    Widget::hstack()
        .sense(Sense::CLICK)
        .focusable(true)
        // Space classifies as `KeyClass::Text`; a focused toggle is not a typing target.
        .input_scope(KeyFilter::TEXT)
}

/// Whether the row was activated: clicked, or Space while focused.
pub(crate) fn activated(ui: &mut Ui, widget: &mut Widget, response: &ResponseState) -> bool {
    let id = widget.resolve(ui);
    response.clicked()
        || (!response.disabled
            && ui.is_focus_within(id)
            && widget.key_pressed(ui, Shortcut::key(Key::Char(' '))))
}

/// Flips `value` when `activated` and returns it, for [`Checkbox`](crate::Checkbox) and
/// [`Switch`](crate::Switch); [`RadioButton`](crate::RadioButton) latches instead.
pub(crate) const fn toggled(activated: bool, value: &mut bool) -> bool {
    if activated {
        *value = !*value;
    }
    *value
}

impl ToggleRow<'_> {
    /// Shared scaffolding behind [`crate::Checkbox`], [`crate::RadioButton`]
    /// and [`crate::Switch`]. `body` receives the box's resolved chrome, as
    /// `Switch` measures its knob inset against the animating stroke width.
    pub(crate) fn record_row(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &Chrome),
    ) -> Response<'_> {
        let Self {
            row: mut widget,
            response,
            label,
            plan,
            gap,
            boxed,
            pill,
        } = self;
        let id = widget.resolve(ui);
        let mut look = widget.wear(ui, plan);
        if let Some(radius) = pill {
            look.chrome.corners = Corners::all(radius);
        }

        widget
            .configure()
            .gap(gap)
            .child_align(Align::v(VAlign::Center));

        widget.record(ui, None, |ui| {
            boxed
                .id(id.with("box"))
                .record(ui, Some(&look.chrome), |ui| body(ui, &look.chrome));

            if !label.is_empty() {
                Text::new(label)
                    .id(id.with("label"))
                    .style(&look.text)
                    .text_align(Align::v(VAlign::Center))
                    .show(ui);
            }
        });

        Response::new(id, ui, response)
    }
}

#[cfg(test)]
mod tests;
