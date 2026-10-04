//! The shared `HStack [box, label]` scaffolding behind the three
//! toggle widgets, and the resolved chrome each hands it.

use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::paint::background::Background;
use crate::primitives::text::text_input::TextInput;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::look_plan::LookPlan;
use crate::widgets::text::Text;

/// What [`ToggleChrome::record_row`] needs from its caller beyond the
/// entry, the label, and the indicator body.
///
/// The theme arrives **planned, not applied**. Each toggle reads its own
/// slot (`theme.checkbox` / `theme.radio` / `theme.switch`) — they share a
/// theme *type* but not a *slot*, and only the caller knows which is its own
/// — so the caller builds the [`LookPlan`] off the same slot the geometry
/// scalars come from. Applying it is the row's own step, and belongs with the
/// rest of the scaffolding: a plan owns everything it carries, so the theme
/// borrow ends at this struct literal and [`Self::record_row`] is free to
/// reborrow the `Ui` mutably.
#[derive(Debug)]
pub(crate) struct ToggleChrome {
    /// The look this row animates toward, off the caller's slot.
    pub(crate) plan: LookPlan,
    /// Gap between the box and the label, off the same slot as `plan`.
    pub(crate) gap: f32,
    /// The box/track child recorded before the label, already sized and
    /// in its layout mode — a square leaf for `Checkbox`/`RadioButton`,
    /// a wide `Canvas` for `Switch`'s track. [`Self::record_row`] only
    /// stamps the id (`<row>.with("box")`) and the resolved chrome onto
    /// it.
    pub(crate) boxed: Widget,
    /// Corner radius forced onto the box chrome, overriding whatever
    /// radius the theme stored. The radio pip and the switch track must
    /// read as pills however they are re-themed; `None` keeps the
    /// theme's own corners (checkbox).
    pub(crate) pill: Option<f32>,
}

impl ToggleChrome {
    /// The widget every toggle row starts from: a horizontal stack that
    /// senses a click, because the whole row — box *and* label — is one
    /// hit target.
    ///
    /// `#[track_caller]` like any other widget constructor, so the id
    /// still resolves to the call site that asked for the widget rather
    /// than to this line.
    #[track_caller]
    pub(crate) fn row() -> Widget {
        Widget::hstack()
            .sense(Sense::CLICK)
            .focusable(true)
            // Space classifies as `KeyClass::Text`, so taking it means
            // claiming that class — right for a focused toggle, which is
            // not a typing target, as for a focused `Expander` header.
            .input_scope(KeyFilter::TEXT)
    }

    /// Whether the row was activated this frame: clicked, or Space pressed
    /// while it holds focus. The click half is already empty on a disabled
    /// row; the key half checks, since keys never pass through that fold.
    pub(crate) fn activated(ui: &mut Ui, widget: &mut Widget, response: &ResponseState) -> bool {
        let id = widget.resolve(ui);
        response.clicked()
            || (!response.disabled
                && ui.is_focus_within(id)
                && widget.key_pressed(ui, Shortcut::key(Key::Char(' '))))
    }

    /// Flip `value` when the row was `activated`, and answer what it now
    /// holds.
    ///
    /// One body for [`Checkbox`](crate::Checkbox) and
    /// [`Switch`](crate::Switch): both bind a `bool` an activation inverts.
    /// [`RadioButton`](crate::RadioButton) latches instead —
    /// re-activating the selected option is a no-op — so it resolves its
    /// own.
    pub(crate) const fn toggled(activated: bool, value: &mut bool) -> bool {
        if activated {
            *value = !*value;
        }
        *value
    }

    /// Shared `HStack [box, label]` scaffolding behind [`crate::Checkbox`],
    /// [`crate::RadioButton`], and [`crate::Switch`]. The three differ only
    /// in the toggle semantics (resolved by the caller before this runs),
    /// the box child, and what `body` paints inside it. Everything
    /// structural — the themed look resolution, the row gap /
    /// cross-centering, the box chrome, the label leaf — lives here.
    ///
    /// The row `HStack` node (sense + salt already set) rides in `widget`,
    /// its probed response in `response`. `body` runs inside the box child
    /// and is handed the box's resolved chrome: `Switch` measures its knob
    /// inset against the *animating* stroke width, which is why the
    /// background is passed in rather than re-derived from the theme.
    pub(crate) fn record_row<'ui>(
        self,
        ui: &'ui mut Ui,
        mut widget: Widget,
        response: ResponseState,
        label: TextInput<'_>,
        body: impl FnOnce(&mut Ui, &Background),
    ) -> Response<'ui> {
        let id = widget.resolve(ui);
        let Self {
            plan,
            gap,
            boxed,
            pill,
        } = self;
        let mut look = plan.apply(ui, &mut widget);
        if let Some(radius) = pill {
            look.background.corners = Corners::all(radius);
        }

        widget
            .configure()
            .gap(gap)
            .child_align(Align::v(VAlign::Center));

        widget.record(ui, None, |ui| {
            boxed
                .id(id.with("box"))
                .record(ui, Some(&look.background), |ui| body(ui, &look.background));

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
