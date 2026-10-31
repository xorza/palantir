//! The scrubbable number field: drag to change, click to type.

use crate::input::key_class::{KeyClass, KeyFilter};
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::{AxisKeys, KeyPair};
use crate::widgets::drag_num::{DragNum, MAX_DECIMALS, Num, StoreRule};
use crate::widgets::text_edit::TextEdit;
use crate::widgets::text_edit::edit_end::EditEnd;
use std::mem;
use std::ops::RangeInclusive;
use std::rc::Rc;

/// One exclusive interaction per [`DragValue`] id: a live [`Scrub`], or an edit whose draft outlives its focus until the chip resolves it.
#[derive(Debug, Default)]
enum DragValueState {
    #[default]
    Idle,
    Scrubbing(Scrub),
    Editing {
        buffer: String,
        original: Num,
    },
}

/// A live scrub: starting value (a [`Num`]), speed and travel. Travel is retained, not the produced value: the stop edge carries no distance, and a deferred caller may have re-seeded the stored value.
#[derive(Clone, Copy, Debug)]
struct Scrub {
    anchor: Num,
    speed: f64,
    travel: f32,
}

impl Scrub {
    fn offset(self) -> f64 {
        f64::from(self.travel) * self.speed
    }
}

/// A numeric field you scrub by dragging horizontally: each pixel of left-button travel changes the value by `speed`, optionally clamped. Binds an `i64` (rounded) or `f64` (snapped to `decimals`), see [`DragNum`]. Chip theme slot `drag_value.chip`.
///
/// With [`Self::editable`] a plain click swaps in an inline `TextEdit` (slot `drag_value.editor`, same box) for exact entry; Enter or click-away commits, Escape restores the opening value. The editor holds the chip's width and scrolls longer values.
///
/// The value is written live; [`ValueResponse`] reports `changed` per differing write and `committed` once per gesture (release, Enter, blur). An undo-aware caller can re-seed every frame and apply on `committed`; the widget re-writes the final value on that frame. A gesture ending while disabled is dropped.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct DragValue<'a> {
    widget: Widget,
    value: DragNum<'a>,
    speed: f64,
    min: f64,
    max: f64,
    decimals: usize,
    suffix: TextInput<'a>,
    editable: bool,
    text_align: Align,
    style: Option<&'a DragValueTheme>,
}

impl<'a> DragValue<'a> {
    /// A scrub chip bound to `value` (`&mut f64` or `&mut i64`). Unbounded until [`Self::range`].
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>) -> Self {
        Self {
            widget: Widget::leaf()
                .focusable(true)
                .input_scope(KeyFilter::TEXT.union(KeyFilter::CARET)),
            value: value.into(),
            speed: 1.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            decimals: 2,
            suffix: TextInput::Borrowed(""),
            editable: false,
            text_align: Align::CENTER,
            style: None,
        }
    }

    /// The value's position inside the chip, and the inline editor's. Default
    /// [`Align::CENTER`].
    pub const fn text_align(mut self, a: Align) -> Self {
        self.text_align = a;
        self
    }

    /// Value change per logical pixel of horizontal drag. Default `1.0`.
    ///
    /// # Panics
    ///
    /// Panics unless `speed` is finite and above zero.
    #[track_caller]
    pub const fn speed(mut self, speed: f64) -> Self {
        self.speed = domain::f64::positive(speed);
        self
    }

    /// Clamps the value into `range`; default unbounded. An end may be infinite and a reversed range is ordered. See [`Slider::new`](crate::Slider::new) for why this is a builder step here.
    ///
    /// # Panics
    ///
    /// Panics if either end is NaN.
    #[track_caller]
    pub const fn range(mut self, range: RangeInclusive<f64>) -> Self {
        assert!(
            !range.start().is_nan() && !range.end().is_nan(),
            "a drag range's ends must not be NaN",
        );
        self.min = *range.start();
        self.max = *range.end();
        self
    }

    /// Digits after the decimal point, for display and the precision a float drag snaps to; keyboard entry stays exact. Ignored by the integer target. Default `2`. `n` is coerced to at most 15, the digits an `f64` holds.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = if n < MAX_DECIMALS { n } else { MAX_DECIMALS };
        self
    }

    /// Text appended after the number, such as a unit.
    pub fn suffix(mut self, text: impl Into<TextInput<'a>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Enables keyboard entry: a click that latches no drag, Enter on the focused chip, or a typed character swaps in an inline `TextEdit`. Enter and click-away commit, Escape reverts. Default off.
    pub const fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// What the widget needs: the scrub drag, plus the click that opens the editor once [`Self::editable`] is on. Folded at [`Self::show`] rather than written by the setter, which would make `editable` depend on chain order.
    fn required_sense(&self) -> Sense {
        if self.editable {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::DRAG
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `drag_value`, covering the chip and the editor.
    pub fn style(mut self, s: impl Into<Option<&'a DragValueTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Records the chip, or its inline editor while open.
    ///
    /// A focused chip is a spin button: Up/Down step by one unit of the last decimal (one for integers), ten with Shift, each a committed edit.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let required = self.required_sense();
        self.configure().add_sense(required);
        let mut response = self.widget.response(ui);
        let id = response.id;

        // Focused + editable + enabled: the editor owns the frame, given the chip's last pre-transform rect (`rect` is post-zoom) to hold its width. Disabled mid-edit falls to the chip path, which discards the draft.
        let focused = ui.focus() == Some(id);
        if self.editable && focused {
            if response.disabled {
                ui.clear_focus();
            } else {
                let editing = matches!(
                    ui.state::<DragValueState>(id),
                    Some(DragValueState::Editing { .. })
                );
                if editing || typed(ui) {
                    return self.show_editing(ui, id, response.layout_rect);
                }
            }
        }

        let rule = StoreRule::new(self.min, self.max, self.decimals);
        let mut changed = false;
        let mut committed = false;

        // Left-button scrub only: other buttons' drags (context menu, canvas pan) must neither write nor commit.
        let drag_started = response.left.drag.started();
        let drag_delta = response.left.drag.delta();
        let drag_stopped = response.left.drag.stopped();
        if drag_started || ui.state::<DragValueState>(id).is_some() {
            ui.with_state::<DragValueState, _>(id, |_, state| {
                if let DragValueState::Editing { buffer, .. } = state {
                    if self.editable && !response.disabled {
                        changed = self.value.parse_from(buffer, rule);
                        committed = true;
                    }
                    *state = DragValueState::Idle;
                }

                if drag_started {
                    *state = DragValueState::Scrubbing(Scrub {
                        anchor: self.value.read(),
                        speed: self.speed,
                        travel: 0.0,
                    });
                }

                let mut stopped = None;
                if let DragValueState::Scrubbing(scrub) = state {
                    if !response.disabled
                        && let Some(delta) = drag_delta
                    {
                        scrub.travel = delta.x;
                        changed |= self.value.commit_drag(scrub.anchor, scrub.offset(), rule);
                    }
                    if drag_stopped {
                        stopped = Some(*scrub);
                    }
                }
                // The stop edge is the commit: the scrub's own travel carries the final value. Released while disabled, the gesture is dropped.
                if let Some(scrub) = stopped {
                    *state = DragValueState::Idle;
                    if !response.disabled {
                        changed |= self.value.commit_drag(scrub.anchor, scrub.offset(), rule);
                        committed = true;
                    }
                }
            });
        }

        if focused && !response.disabled {
            let travel = KEYS.read(&mut self.widget, ui);
            if travel.steps != 0 {
                // Through the drag's commit, so an integer adds whole steps to its exact value.
                let by = f64::from(travel.steps) * self.value.unit(self.decimals);
                changed |= self.value.commit_drag(self.value.read(), by, rule);
                committed = true;
            }
        }

        let enter = focused && self.widget.shortcut_pressed(ui, Key::Enter);
        if self.editable && !response.disabled && (response.clicked() || enter) {
            ui.set_focus(id);
            response.state.focused = true;
            ui.with_state::<DragValueState, _>(id, |_, s| {
                *s = DragValueState::Editing {
                    buffer: self.value.edit_string(),
                    original: self.value.stored(),
                }
            });
        }

        let text = match &self.suffix {
            TextInput::Borrowed(suffix) => self.value.label(ui, self.decimals, suffix),
            TextInput::Owned(suffix) => self.value.label(ui, self.decimals, suffix),
            TextInput::Interned(suffix) => {
                let (suffix, value, decimals) = (*suffix, &self.value, self.decimals);
                ui.with_state::<SuffixScratch, _>(id, |ui, scratch| {
                    scratch.0.clear();
                    scratch.0.push_str(ui.text(suffix));
                    value.label(ui, decimals, &scratch.0)
                })
            }
        };

        let theme = ui.theme();
        let chip = &self.style.unwrap_or(&theme.drag_value).chip;
        let plan = chip.plan(&response, Selection::Unselected, theme.text);
        let look = self.widget.wear(ui, plan);

        self.widget.record(ui, Some(&look.chrome), |ui| {
            ui.add_shape(
                Shape::text(text, look.text.font())
                    .color(look.text.color)
                    .wrap(TextWrap::Truncate)
                    .align(self.text_align),
            );
        });
        ValueResponse {
            response: Response::new(ui, response),
            changed,
            committed,
        }
    }

    /// Edit mode: the inline `TextEdit` over the same `id`, parsed back each frame, blurring on Enter. Escape restores the opening value; click-away leaves the draft for the chip path.
    fn show_editing(
        mut self,
        ui: &mut Ui,
        id: WidgetId,
        prev_rect: Option<Rect>,
    ) -> ValueResponse<'_> {
        // The editor must wear the chip's box or the field resizes on click: `DragValueTheme::from_chip` mirrors the chip padding onto `drag_value.editor`, whereas an unstyled `TextEdit` inherits `theme.text_edit`. A handle avoids copying ~700 bytes of `TextEditTheme`.
        let ui_theme = Rc::clone(ui.theme());
        let editor = match self.style {
            Some(s) => &s.editor,
            None => &ui_theme.drag_value.editor,
        };
        // Hold the editor at the chip's last width: as a `Scroll` field it reports zero content width, so a plain cap would collapse to `min_size`. `Fixed` (floored at `min_size.w`) makes long values scroll. Before the first chip frame, use the field's own sizing.
        let min_size = self.widget.authored_min_size().unwrap_or(Size::ZERO);
        let sizes = self.widget.authored_size().unwrap_or_default();
        let held_w = prev_rect.map(|r| Sizing::fixed(r.size.w.max(min_size.w)));
        let width = held_w.unwrap_or_else(|| sizes.w());
        let (mut buffer, original) =
            match ui.with_state::<DragValueState, _>(id, |_, s| mem::take(s)) {
                DragValueState::Editing { buffer, original } => (buffer, original),
                DragValueState::Idle | DragValueState::Scrubbing(_) => {
                    (self.value.edit_string(), self.value.stored())
                }
            };
        let ended = {
            let edit = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(self.text_align)
                .select_all_on_focus(true)
                .style(editor)
                .size((width, sizes.h()))
                .min_size(min_size)
                .max_size(self.widget.authored_max_size().unwrap_or(Size::INF));
            edit.adopt_placement(&self.widget).show(ui).ended
        };
        let changed = if ended == Some(EditEnd::Canceled) {
            self.value.restore(original)
        } else {
            self.value
                .parse_from(&buffer, StoreRule::new(self.min, self.max, self.decimals))
        };
        ui.with_state::<DragValueState, _>(id, |_, s| {
            *s = if ended.is_some() {
                DragValueState::Idle
            } else {
                DragValueState::Editing { buffer, original }
            }
        });
        if ended == Some(EditEnd::Canceled) {
            ui.set_focus(id);
        }
        ValueResponse {
            response: Response::lazy(id, ui),
            changed,
            committed: ended == Some(EditEnd::Submitted),
        }
    }
}

/// Up and Down step the value by one unit of its last shown digit; Shift steps ten.
const KEYS: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowDown,
        forward: Key::ArrowUp,
    }],
    page: None,
    ends: None,
};

impl Configure for DragValue<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// Whether a character was typed into the focused chip this frame; Space is excluded, as a spin button doesn't type it.
fn typed(ui: &Ui) -> bool {
    ui.keyboard_events().iter().any(|press| {
        KeyClass::of(*press) == KeyClass::Text
            && !press.text.is_empty()
            && press.text.as_str() != " "
    })
}

/// An interned suffix's characters copied out of the arena for formatting; retained, so a steady suffix allocates once.
#[derive(Debug, Default)]
struct SuffixScratch(String);

#[cfg(test)]
mod tests;
