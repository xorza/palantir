use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::{Response, WidgetEntry, button_look, enter_widget};
use std::ops::RangeInclusive;

/// Per-id drag anchor: the value captured when a drag latches, so
/// cumulative `drag_delta` offsets from a stable base rather than
/// accumulating frame-to-frame rounding.
#[derive(Default)]
struct DragAnchor {
    value: f32,
}

/// A numeric field you scrub by dragging horizontally (Blender / egui
/// style): each pixel of horizontal travel changes the value by `speed`,
/// optionally clamped to a range. Renders as a button-styled chrome
/// (theme slot `button`) with the formatted number centered inside.
///
/// Text entry (click-to-type) is intentionally not wired yet — this is
/// the drag-to-scrub core; pair it with a [`crate::Slider`] or a
/// [`crate::TextEdit`] when keyboard input is needed.
pub struct DragValue<'a> {
    element: Element,
    value: &'a mut f32,
    speed: f32,
    min: f32,
    max: f32,
    decimals: usize,
    suffix: &'static str,
    style: Option<ButtonTheme>,
}

impl<'a> DragValue<'a> {
    #[track_caller]
    pub fn new(value: &'a mut f32) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.flags.set_sense(Sense::DRAG);
        Self {
            element,
            value,
            speed: 1.0,
            min: f32::NEG_INFINITY,
            max: f32::INFINITY,
            decimals: 2,
            suffix: "",
            style: None,
        }
    }

    /// Value change per logical pixel of horizontal drag. Default `1.0`.
    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Clamp the value into `range`. Default unbounded.
    pub fn range(mut self, range: RangeInclusive<f32>) -> Self {
        self.min = *range.start();
        self.max = *range.end();
        self
    }

    /// Digits after the decimal point in the display. Default `2`.
    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Static text appended after the number (e.g. `"px"`, `"%"`).
    pub fn suffix(mut self, s: &'static str) -> Self {
        self.suffix = s;
        self
    }

    /// Override the chrome theme. `None` (default) inherits
    /// [`crate::Theme::button`].
    pub fn style(mut self, s: ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut element = self.element;
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &element);

        // Capture the value when the drag latches, then offset by the
        // cumulative travel each frame.
        if state.drag_started() {
            ui.state_mut::<DragAnchor>(id).value = *self.value;
        }
        if !state.disabled
            && state.dragged()
            && let Some(delta) = state.drag_delta()
        {
            let anchor = ui.state_mut::<DragAnchor>(id).value;
            *self.value = apply_drag(anchor, delta.x, self.speed, self.min, self.max);
        }

        let text = ui.fmt(format_args!(
            "{:.*}{}",
            self.decimals, *self.value, self.suffix
        ));

        let look = button_look(ui, id, &mut element, state, self.style.as_ref());

        ui.node(id, element, Some(&look.background), |ui| {
            ui.add_shape(Shape::Text {
                local_origin: None,
                text,
                brush: look.text.color.into(),
                font_size_px: look.text.font_size_px,
                line_height_px: look.line_height_px(),
                wrap: TextWrap::Truncate,
                align: Align::CENTER,
                family: look.text.family,
            });
        });
        Response::eager(id, ui, raw_state)
    }
}

impl Configure for DragValue<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// New value after a drag of `delta_x` pixels at `speed` per pixel,
/// offset from the latched `anchor` and clamped into `[min, max]`
/// (tolerating a reversed pair).
fn apply_drag(anchor: f32, delta_x: f32, speed: f32, min: f32, max: f32) -> f32 {
    (anchor + delta_x * speed).clamp(min.min(max), min.max(max))
}

#[cfg(test)]
mod tests {
    use super::apply_drag;

    #[test]
    fn apply_drag_offsets_from_anchor_by_speed() {
        // 10 px at speed 0.5 → +5 from the anchor.
        assert!(
            (apply_drag(20.0, 10.0, 0.5, f32::NEG_INFINITY, f32::INFINITY) - 25.0).abs() < 1e-6
        );
        // Negative travel decreases.
        assert!(
            (apply_drag(20.0, -8.0, 1.0, f32::NEG_INFINITY, f32::INFINITY) - 12.0).abs() < 1e-6
        );
        // Zero speed pins the value at the anchor regardless of travel.
        assert!((apply_drag(7.0, 100.0, 0.0, f32::NEG_INFINITY, f32::INFINITY) - 7.0).abs() < 1e-6);
    }

    #[test]
    fn apply_drag_clamps_into_range() {
        // Drives past the top — clamps to max.
        assert!((apply_drag(8.0, 100.0, 1.0, 0.0, 10.0) - 10.0).abs() < 1e-6);
        // Drives below the bottom — clamps to min.
        assert!((apply_drag(2.0, -100.0, 1.0, 0.0, 10.0) - 0.0).abs() < 1e-6);
        // Reversed bounds clamp identically.
        assert!((apply_drag(2.0, -100.0, 1.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
    }
}
