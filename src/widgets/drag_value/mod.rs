//! The scrubbable number field: drag to change, click to type.

use crate::input::key_class::{KeyClass, KeyFilter};
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::text::interned_str::InternedStr;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::drag_num::DragNum;
use crate::widgets::drag_num::Num;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::drag_value::DragValueTheme;
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

#[derive(Clone, Copy, Debug)]
struct EditEnd {
    submitted: bool,
    canceled: bool,
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
            style: None,
        }
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

    /// Digits after the decimal point, for display and the precision a float drag snaps to; keyboard entry stays exact. Ignored by the integer target. Default `2`.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Text appended after the number, such as a unit.
    pub fn suffix(mut self, text: impl Into<TextInput<'a>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Enables keyboard entry: a click that latches no drag, Enter on the focused chip, or a typed character swaps in an inline `TextEdit`. Enter and click-away commit, Escape reverts. Default off.
    pub const fn editable(mut self, on: bool) -> Self {
        self.editable = on;
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
        let id = self.widget.resolve(ui);

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
                        changed = self.value.parse_from(buffer, self.min, self.max);
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
                        changed |= self.value.commit_drag(
                            scrub.anchor,
                            scrub.offset(),
                            self.decimals,
                            self.min,
                            self.max,
                        );
                    }
                    if drag_stopped {
                        stopped = Some(*scrub);
                    }
                }
                // The stop edge is the commit: the scrub's own travel carries the final value. Released while disabled, the gesture is dropped.
                if let Some(scrub) = stopped {
                    *state = DragValueState::Idle;
                    if !response.disabled {
                        changed |= self.value.commit_drag(
                            scrub.anchor,
                            scrub.offset(),
                            self.decimals,
                            self.min,
                            self.max,
                        );
                        committed = true;
                    }
                }
            });
        }

        if focused && !response.disabled {
            let step = match self.value.read() {
                Num::I64(_) => 1.0,
                Num::F64(_) => 1.0 / 10f64.powi(self.decimals.min(15) as i32),
            };
            let mut moved = 0.0;
            for (key, sign) in [(Key::ArrowUp, 1.0), (Key::ArrowDown, -1.0)] {
                let coarse = self
                    .widget
                    .key_pressed(ui, Shortcut::new(ShortcutMods::SHIFT, key));
                let plain = self.widget.key_pressed(ui, Shortcut::key(key));
                if coarse {
                    moved += sign * step * 10.0;
                } else if plain {
                    moved += sign * step;
                }
            }
            if moved != 0.0 {
                let to = self.value.read().widen() + moved;
                changed |= self
                    .value
                    .commit_value(to, self.decimals, self.min, self.max);
                committed = true;
            }
        }

        let enter = focused && self.widget.key_pressed(ui, Shortcut::key(Key::Enter));
        if self.editable && !response.disabled && (response.clicked() || enter) {
            ui.set_focus(id);
            response.focused = true;
            ui.with_state::<DragValueState, _>(id, |_, s| {
                *s = DragValueState::Editing {
                    buffer: self.value.edit_string(),
                    original: self.value.read(),
                }
            });
        }

        let text = match &self.suffix {
            TextInput::Borrowed(suffix) => label(ui, &self.value, self.decimals, suffix),
            TextInput::Owned(suffix) => label(ui, &self.value, self.decimals, suffix),
            TextInput::Interned(suffix) => {
                let (suffix, value, decimals) = (*suffix, &self.value, self.decimals);
                ui.with_state::<SuffixScratch, _>(id, |ui, scratch| {
                    scratch.0.clear();
                    scratch.0.push_str(ui.text(suffix));
                    label(ui, value, decimals, &scratch.0)
                })
            }
        };

        let theme = ui.theme();
        let chip = &self.style.unwrap_or(&theme.drag_value).chip;
        let look = chip
            .plan(&response, (), theme.text)
            .apply(ui, &mut self.widget);

        self.widget.record(ui, Some(&look.background), |ui| {
            ui.add_shape(
                Shape::text(text, look.text.font())
                    .color(look.text.color)
                    .wrap(TextWrap::Truncate)
                    .align(Align::CENTER),
            );
        });
        ValueResponse {
            response: Response::new(id, ui, response),
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
        let width = held_w.unwrap_or(sizes.w());
        let (mut buffer, original) =
            match ui.with_state::<DragValueState, _>(id, |_, s| mem::take(s)) {
                DragValueState::Editing { buffer, original } => (buffer, original),
                DragValueState::Idle | DragValueState::Scrubbing(_) => {
                    (self.value.edit_string(), self.value.read())
                }
            };
        let ended = {
            let edit = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(Align::CENTER)
                .select_all_on_focus(true)
                .style(editor)
                .size((width, sizes.h()))
                .min_size(min_size)
                .max_size(self.widget.authored_max_size().unwrap_or(Size::INF));
            let resp = edit.adopt_placement(&self.widget).show(ui);
            EditEnd {
                submitted: resp.submitted,
                canceled: resp.canceled,
            }
        };
        let changed = if ended.canceled {
            self.value.restore(original)
        } else {
            self.value.parse_from(&buffer, self.min, self.max)
        };
        ui.with_state::<DragValueState, _>(id, |_, s| {
            *s = if ended.submitted || ended.canceled {
                DragValueState::Idle
            } else {
                DragValueState::Editing { buffer, original }
            }
        });
        if ended.canceled {
            ui.set_focus(id);
        }
        ValueResponse {
            response: Response::lazy(id, ui),
            changed,
            committed: ended.submitted,
        }
    }
}

impl Configure for DragValue<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
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

/// The chip's text: `value` at `decimals` places, then `suffix`.
fn label(ui: &mut Ui, value: &DragNum<'_>, decimals: usize, suffix: &str) -> InternedStr {
    match value {
        DragNum::I64(v) => ui.fmt(format_args!("{}{suffix}", **v)),
        DragNum::F64(v) => ui.fmt(format_args!("{:.*}{suffix}", decimals, **v)),
    }
}

/// An interned suffix's characters copied out of the arena for formatting; retained, so a steady suffix allocates once.
#[derive(Debug, Default)]
struct SuffixScratch(String);

#[cfg(test)]
mod tests;
