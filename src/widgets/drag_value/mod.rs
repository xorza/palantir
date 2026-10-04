//! The scrubbable number field — drag to change, click to type. Holds the
//! widget, the integer-or-float target it writes through, the retained
//! drag and edit state, and what a frame of either reports.

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

/// One mutually exclusive interaction per [`DragValue`] id: a live
/// [`Scrub`], or an edit whose draft outlives its focus until the chip
/// can resolve it.
#[derive(Debug, Default)]
enum DragValueState {
    #[default]
    Idle,
    Scrubbing(Scrub),
    /// `original` is the value the edit opened on, which Escape restores:
    /// the buffer is parsed live, so by then the bound value holds the
    /// typed text.
    Editing {
        buffer: String,
        original: Num,
    },
}

/// A live scrub: the value it began on, how fast it moves, and how far
/// it has come.
///
/// The anchor is a [`Num`], since the gesture writes back through it.
///
/// Travel is retained rather than the value it produced. The stop edge
/// carries no drag distance, so the release frame derives its result
/// from the anchor again — reading the last stored value back instead
/// would find whatever a deferred caller re-seeded there.
#[derive(Clone, Copy, Debug)]
struct Scrub {
    anchor: Num,
    speed: f64,
    /// Cumulative pointer travel of the last frame that wrote, in
    /// logical pixels.
    travel: f32,
}

/// How the inline editor's frame ended the edit, if it did.
#[derive(Clone, Copy, Debug)]
struct EditEnd {
    submitted: bool,
    canceled: bool,
}

impl Scrub {
    /// How far the anchor has moved, in value units.
    fn offset(self) -> f64 {
        f64::from(self.travel) * self.speed
    }
}

/// A numeric field you scrub by dragging horizontally (Blender / egui
/// style): each pixel of horizontal left-button travel changes the value
/// by `speed`, optionally clamped to a range. Binds either an `i64` or an
/// `f64` (see [`DragNum`]) — the integer target rounds to the nearest whole
/// step and a float drag snaps to `decimals`. Renders as a button-styled
/// chip (theme slot `drag_value.chip`) with the formatted number centered
/// inside.
///
/// With [`Self::editable`] the widget is a complete numeric editor: a plain
/// click (no drag) focuses it and swaps the chip for an inline `TextEdit`
/// (theme slot `drag_value.editor`, same box as the chip) for exact keyboard
/// entry; Enter or clicking away commits and returns to the scrub chip, and
/// Escape returns to it with the value the edit opened on. The editor holds the
/// chip's width and **scrolls** a longer full-precision value inside it, so it
/// stays put even in a content-hugging parent.
///
/// The value is written live — every scrub step and edit-mode reparse lands
/// in the bound target — and [`ValueResponse`] reports both grains:
/// `changed` per differing write, `committed` once per finished gesture
/// (drag release, Enter, blur). An undo-aware caller can ignore `changed`,
/// re-seed the bound value from its canonical source every frame, and apply
/// it only on `committed`: the widget re-writes the gesture's final value on
/// the commit frame, so the deferred caller still observes it. A gesture
/// that ends while the widget is disabled (or, for a pending edit, no
/// longer editable) is dropped, not committed.
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
    /// A scrub chip bound to `value`, which takes a `&mut f64` or a
    /// `&mut i64`. Unbounded until [`Self::range`].
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>) -> Self {
        Self {
            // A Tab stop, as WAI-ARIA's spin button: the arrows step it, and
            // typing and Enter open the editor — the `CARET` and `TEXT`
            // classes.
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

    /// Value change per logical pixel of horizontal drag, *positive*.
    /// Default `1.0`.
    ///
    /// # Panics
    ///
    /// Panics unless `speed` is finite and above zero.
    #[track_caller]
    pub const fn speed(mut self, speed: f64) -> Self {
        self.speed = domain::f64::positive(speed);
        self
    }

    /// Clamp the value into `range`. Default unbounded.
    ///
    /// A builder step here and a constructor argument on
    /// [`Slider::new`](crate::Slider::new), which says why.
    ///
    /// An end may be infinite — that is the unbounded default — and a
    /// reversed range is ordered.
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

    /// Digits after the decimal point. Governs both the scrub display *and*
    /// the precision a float drag snaps to (so dragging never stores a long
    /// tail — the value matches what's shown). Keyboard entry stays exact.
    /// Ignored by the integer target. Default `2`.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Text appended after the number — a unit (`"px"`, `"%"`), or
    /// whatever a locale table hands over: borrowed, owned, interned or
    /// `fmt!` output, as every widget's text.
    pub fn suffix(mut self, text: impl Into<TextInput<'a>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Enable keyboard entry alongside drag-to-scrub. A click that latches
    /// no drag, Enter on the focused chip, or a character typed into it
    /// swaps the chip for an inline `TextEdit` — a typed character
    /// replaces the value, as a spin button's does. Enter and click-away
    /// commit, Escape reverts, and either leaves the chip focused.
    /// Default off.
    pub const fn editable(mut self, on: bool) -> Self {
        self.editable = on;
        self
    }

    /// What the widget cannot work without: the scrub drag always, and
    /// the click that opens the inline editor once [`Self::editable`] is
    /// on.
    ///
    /// Read at [`Self::show`] and folded over whatever the caller sensed,
    /// rather than written into the node by the setter. A setter would
    /// make `editable` depend on the order it was chained in — it would
    /// drop a `sense` set before it, keep the click after an
    /// `editable(false)`, and lose to a `sense` set after it.
    fn required_sense(&self) -> Sense {
        if self.editable {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::DRAG
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `drag_value`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// Covers both modes at once — the scrub chip and the inline editor.
    pub fn style(mut self, s: impl Into<Option<&'a DragValueTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the chip, or its inline editor while one is open.
    ///
    /// A focused chip is a spin button: Up and Down step the value by one
    /// unit of its last decimal — one, for an integer — and ten units
    /// with Shift, each step a committed edit. Focus alone opens no
    /// editor, so the press that starts a scrub may focus the chip.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let required = self.required_sense();
        self.configure().add_sense(required);
        let mut response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        // Focused + editable + enabled: the inline text editor owns the
        // frame. Pass the chip's last *pre-transform* rect (logical px,
        // matching min/max_size) so the editor holds that width instead of
        // growing a content-hugging parent to fit the full-precision value —
        // `rect` is post-zoom and would mismatch the sizing units under a
        // scaled canvas. Disabled mid-edit falls through to the chip path,
        // which kicks focus out and discards the pending draft below.
        let focused = ui.focus() == Some(id);
        if self.editable && focused {
            if response.disabled {
                ui.clear_focus();
            } else {
                // An open draft keeps the editor; a character typed into
                // the chip opens it this frame, so the editor takes that
                // character itself and it replaces the selected value.
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

        // Left-button scrub only — a right/middle drag is someone else's
        // gesture (context menu, canvas pan) and must neither write nor
        // commit. Capture the value + speed when the drag latches, then
        // offset by the cumulative travel each frame and commit
        // (snap / round / clamp). One state probe resolves a pending edit,
        // begins a new scrub, and advances or finishes an existing scrub.
        let drag_started = response.left.drag.started();
        let drag_delta = response.left.drag.delta();
        let drag_stopped = response.left.drag.stopped();
        // Probed first, so a chip that never scrubbed or edited stores no
        // row.
        if drag_started || ui.state::<DragValueState>(id).is_some() {
            ui.with_state::<DragValueState, _>(id, |_, state| {
                // A click-away reaches the chip with the edit draft still
                // present. Resolve it while editable and enabled, otherwise drop
                // it so a later focus cannot replay stale input.
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
                // The stop edge is the commit: the drag state is already gone on
                // this frame, so the scrub's own travel carries the final value.
                // Released while disabled, the gesture is dropped instead.
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
            // Every chord sampled: `key_pressed` also keeps it subscribed
            // for the wake gate.
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

        // A plain enabled click (no drag latched), or Enter on the focused
        // chip, opens keyboard entry on the next frame — Enter so the
        // editor does not take the Enter that opened it as its submit.
        let enter = focused && self.widget.key_pressed(ui, Shortcut::key(Key::Enter));
        if self.editable && !response.disabled && (response.clicked() || enter) {
            ui.set_focus(id);
            // The probed snapshot predates the request, so without this
            // the response denies the focus the widget just took.
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
            // The arena cannot be read while it is written, so an interned
            // suffix goes through a retained copy.
            TextInput::Interned(suffix) => {
                let (suffix, value, decimals) = (*suffix, &self.value, self.decimals);
                ui.with_state::<SuffixScratch, _>(id, |ui, scratch| {
                    scratch.0.clear();
                    scratch.0.push_str(ui.text(suffix));
                    label(ui, value, decimals, &scratch.0)
                })
            }
        };

        // The chip half of the bundle — the same one the edit mode's editor
        // takes its half from, so the two modes stay in sync under a global
        // restyle.
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

    /// Edit mode: render the inline `TextEdit` over the same `id`, centered
    /// and same-styled as the chip (its box matches by theme, not by
    /// measuring the chip), parse the buffer back into the value each frame,
    /// and blur on Enter. Escape blurs itself and restores the value the edit
    /// opened on; a click-away blurs itself and the chip path resolves the
    /// pending [`DragValueState::Editing`] draft.
    fn show_editing(
        mut self,
        ui: &mut Ui,
        id: WidgetId,
        prev_rect: Option<Rect>,
    ) -> ValueResponse<'_> {
        // The editor has to wear the chip's box or the field resizes the moment
        // it is clicked. `DragValueTheme::from_chip` mirrors the chip's padding
        // onto `drag_value.editor` for exactly that; an *unstyled* `TextEdit`
        // inherits `theme.text_edit` instead — a standalone field's box, whose
        // padding is not the chip's — so the bundle has to be handed over
        // rather than left to the field's own default.
        //
        // Held as a handle, because the borrow has to outlive the `&mut Ui`
        // the field is shown with — a refcount bump rather than the ~700-byte
        // `TextEditTheme` copy a plain borrow would have forced.
        let ui_theme = Rc::clone(ui.theme());
        let editor = match self.style {
            Some(s) => &s.editor,
            None => &ui_theme.drag_value.editor,
        };
        // Hold the editor at exactly the width the chip occupied last frame.
        // The chip shows `decimals`-rounded text; the editor shows every digit
        // and, as a `Scroll` field, reports zero content width — so nothing
        // pulls a `Fill` field up to the chip's width and a plain cap would let
        // it collapse to `min_size`. Pin the width with `Fixed` (floored at
        // `min_size.w`) so a long value scrolls inside the chip's box instead
        // of growing a content-hugging row. Before the first chip frame gives
        // us a width to hold, fall back to the field's own width sizing.
        let min_size = self.widget.authored_min_size().unwrap_or(Size::ZERO);
        let sizes = self.widget.authored_size().unwrap_or_default();
        let held_w = prev_rect.map(|r| Sizing::fixed(r.size.w.max(min_size.w)));
        let width = held_w.unwrap_or(sizes.w());
        // Entry replaces any scrub state atomically, so its later release
        // cannot overwrite the typed result. Existing edit frames move the
        // same String through TextEdit without allocating a new buffer.
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
            // The chip's placement has to survive the swap or the field
            // visibly jumps mid-interaction. Its size does not travel with
            // it: the width is pinned above to the chip's last rect, so a
            // long value scrolls instead of growing the row, and
            // `DragValueTheme::from_chip` mirrors the chip's padding onto
            // the editor.
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
        // The chip keeps focus once the edit ends, as a spin button does,
        // so the keyboard goes on from it; Escape blurred the editor.
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

/// The chip's text: `value` at `decimals` places, then `suffix`.
/// Whether a character was typed into the focused chip this frame — text
/// a press produced, Space aside, which a spin button does not type.
fn typed(ui: &Ui) -> bool {
    ui.keyboard_events().iter().any(|press| {
        KeyClass::of(*press) == KeyClass::Text
            && !press.text.is_empty()
            && press.text.as_str() != " "
    })
}

fn label(ui: &mut Ui, value: &DragNum<'_>, decimals: usize, suffix: &str) -> InternedStr {
    match value {
        DragNum::I64(v) => ui.fmt(format_args!("{}{suffix}", **v)),
        DragNum::F64(v) => ui.fmt(format_args!("{:.*}{suffix}", decimals, **v)),
    }
}

/// An interned suffix's characters, copied out of the arena so the label
/// can be formatted into it. Retained, so a steady suffix allocates once.
#[derive(Debug, Default)]
struct SuffixScratch(String);

#[cfg(test)]
mod tests;
