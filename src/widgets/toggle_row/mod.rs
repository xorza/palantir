//! The shared `HStack [box, label]` behind the toggle widgets.

use crate::input::sense::Sense;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::text::text_input::TextInput;
use crate::theme::Theme;
use crate::theme::check_state::CheckState;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;
use std::rc::Rc;

/// What [`Checkbox`](crate::Checkbox), [`RadioButton`](crate::RadioButton) and
/// [`Switch`](crate::Switch) share: one click target over a box and a label, its label and its
/// theme override. Each widget owns one and finishes through [`Self::show`] with its
/// [`ToggleKind`].
#[derive(Debug)]
pub(crate) struct ToggleRow<'a> {
    pub(crate) widget: Widget,
    /// The text right of the box; empty records no text child.
    pub(crate) label: TextInput<'a>,
    pub(crate) style: Option<&'a ToggleTheme>,
}

/// What sets one toggle apart from its siblings: how an activation moves its bound value, its
/// slot, its box and what paints inside the box.
pub(crate) trait ToggleKind {
    /// Moves the bound value on this frame's activation.
    fn settle(self, activated: bool) -> Settled;
    /// The slot read when the caller set no style.
    fn slot(theme: &Theme) -> &ToggleTheme;
    /// The box child, sized from `slot`; [`ToggleRow::show`] stamps its id and chrome onto it.
    fn boxed(slot: &ToggleTheme) -> Widget;
    /// The corner radius forced onto the box chrome, so the radio pip and the switch track read
    /// as pills whatever radius a theme sets; `None` keeps the theme's.
    fn pill(slot: &ToggleTheme) -> Option<f32>;
    /// Paints inside the box of the toggle `id`. `chrome` is the box's resolved chrome, as the
    /// switch insets its knob by the animating border width.
    fn paint(ui: &mut Ui, id: WidgetId, slot: &ToggleTheme, chrome: &Chrome, checked: bool);
}

/// What an activation did to a toggle's bound value.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled {
    /// The box shows checked this frame.
    pub(crate) checked: bool,
    /// The bound value moved.
    pub(crate) changed: bool,
}

impl Settled {
    /// Flips `value` on an activation, for the checkbox and the switch.
    pub(crate) const fn flip(value: &mut bool, activated: bool) -> Self {
        if activated {
            *value = !*value;
        }
        Self {
            checked: *value,
            changed: activated,
        }
    }
}

impl ToggleRow<'_> {
    /// A focusable row that senses a click, as the box and the label are one hit target.
    #[track_caller]
    pub(crate) fn new() -> Self {
        Self {
            widget: Widget::hstack().sense(Sense::CLICK).focusable(true),
            label: TextInput::default(),
            style: None,
        }
    }

    /// Records the row for `kind`. A click or Space activates it, and a move commits at once
    /// (`committed == changed`).
    pub(crate) fn show<K: ToggleKind>(self, ui: &mut Ui, kind: K) -> ValueResponse<'_> {
        let Self {
            mut widget,
            label,
            style,
        } = self;
        let response = widget.activation_response(ui, Activation::Toggle);
        let id = response.id;
        let Settled { checked, changed } = kind.settle(response.clicked());

        // An `Rc` bump, so the slot outlives the `&mut Ui` the record takes.
        let theme = Rc::clone(ui.theme());
        let slot = style.unwrap_or_else(|| K::slot(&theme));
        let plan = slot.plan(&response, CheckState::from(checked), theme.text);
        let mut look = widget.wear(ui, plan);
        if let Some(radius) = K::pill(slot) {
            look.chrome.corners = Corners::all(radius);
        }
        widget
            .configure()
            .default_gap(slot.gap)
            .default_child_align(Align::v(VAlign::Center));

        widget.record(ui, None, |ui| {
            K::boxed(slot)
                .id(id.with("box"))
                .record(ui, Some(&look.chrome), |ui| {
                    K::paint(ui, id, slot, &look.chrome, checked);
                });
            if !label.is_empty() {
                Text::new(label)
                    .id(id.with("label"))
                    .style(&look.text)
                    .text_align(Align::v(VAlign::Center))
                    .show(ui);
            }
        });
        ValueResponse {
            response: Response::new(ui, response),
            changed,
            committed: changed,
        }
    }
}

#[cfg(test)]
mod tests;
