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
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::{self, AxisEnd, AxisKeys, Travel};
use crate::widgets::bound_num::{BoundNum, MAX_DECIMALS, Num, StoreRule};
use crate::widgets::text_edit::TextEdit;
use crate::widgets::text_edit::edit_end::EditEnd;
use crate::widgets::text_edit::text_layout;
use std::mem;
use std::ops::RangeInclusive;

/// One exclusive interaction per [`DragValue`] id: a live [`Scrub`], or an edit whose draft outlives its focus until the chip resolves it.
#[derive(Debug, Default)]
enum DragValueState {
    #[default]
    Idle,
    Scrubbing(Scrub),
    /// [`DragValue::start_editing`] asked for the editor, which the next show seeds from the
    /// bound value.
    Opening,
    Editing {
        buffer: String,
        original: Num,
        /// How [`DragValue::stop_editing`] ended the edit, which the next chip frame resolves.
        end: Option<EditEnd>,
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

/// A numeric field you scrub by dragging horizontally: each pixel of left-button travel changes the value by `speed`, optionally clamped. Binds any [`BoundNum`]: an integer moves by whole steps, a float snaps to `decimals`. Theme slot `drag_value`.
///
/// Unless [`Self::editable`] is off, a plain click swaps in an inline `TextEdit` ([`DragValueTheme::editor`], same looks and box) for exact entry; Enter or click-away commits, Escape restores the opening value. The editor holds the chip's width and scrolls longer values.
///
/// The value is written live; [`ValueResponse`] reports `changed` per differing write and `committed` once per gesture (release, Enter, blur). An undo-aware caller can re-seed every frame and apply on `committed`; the widget re-writes the final value on that frame. A gesture ending while disabled is dropped.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct DragValue<'a> {
    widget: Widget,
    value: BoundNum<'a>,
    speed: f64,
    min: f64,
    max: f64,
    step: Option<f64>,
    decimals: usize,
    suffix: TextInput<'a>,
    editable: bool,
    text_align: Align,
    style: Option<&'a DragValueTheme>,
}

impl<'a> DragValue<'a> {
    /// A scrub chip bound to `value`, any [`BoundNum`]. Unbounded until [`Self::range`].
    #[track_caller]
    pub fn new(value: impl Into<BoundNum<'a>>) -> Self {
        Self {
            widget: Widget::leaf().focusable(true).input_scope(
                KeyFilter::TEXT
                    .union(KeyFilter::ENTER)
                    .union(AxisKeys::SCOPE),
            ),
            value: value.into(),
            speed: 1.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            step: None,
            decimals: 2,
            suffix: TextInput::Borrowed(""),
            editable: true,
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

    /// Clamps the value into `range`; default unbounded. An end may be infinite and a reversed
    /// range is ordered. See [`Slider::new`](crate::Slider::new) for why this is a builder step
    /// here.
    ///
    /// # Panics
    ///
    /// Panics if either end is NaN.
    #[track_caller]
    pub const fn range(mut self, range: RangeInclusive<f64>) -> Self {
        let bounds = domain::f64::bounds(range);
        self.min = *bounds.start();
        self.max = *bounds.end();
        self
    }

    /// Snap the value to multiples of `step` from the range's `min`, or from zero while `min` is
    /// unbounded, as [`Slider::step`](crate::Slider::step) does: a scrub, an arrow and typed
    /// entry all land on the grid, and an arrow moves one `step`. Continuous by default.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is finite and greater than zero.
    #[track_caller]
    pub const fn step(mut self, step: f64) -> Self {
        self.step = Some(domain::f64::positive(step));
        self
    }

    /// Digits after the decimal point, for display and the precision a float drag snaps to; keyboard entry stays exact unless a [step](Self::step) is set. Ignored by the integer target. Default `2`. `n` is coerced to at most 15, the digits an `f64` holds.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = if n < MAX_DECIMALS { n } else { MAX_DECIMALS };
        self
    }

    /// Text appended after the number, such as a unit.
    pub fn suffix(mut self, text: impl Into<TextInput<'a>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Enables keyboard entry: a click that latches no drag, Enter on the focused chip, or a typed character swaps in an inline `TextEdit`. Enter and click-away commit, Escape reverts. Default on, as `EditableLabel::editable`.
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

    /// Per-instance override of [`Theme::drag_value`](crate::Theme::drag_value), covering the chip and the editor.
    pub fn style(mut self, s: impl Into<Option<&'a DragValueTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Records the chip, or its inline editor while open.
    ///
    /// A focused chip is a spin button: Up/Down step by the [step](Self::step), else one unit of
    /// the last decimal (one for integers), ten with Shift; Page Up/Down step ten; Home/End jump
    /// to a bounded end of the range. Each is a committed edit.
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
                    Some(DragValueState::Opening | DragValueState::Editing { end: None, .. })
                );
                if editing || typed(ui) {
                    return self.show_editing(ui, id, response.layout_rect);
                }
            }
        }

        let rule = self.rule();
        let mut changed = false;
        let mut committed = false;

        // Left-button scrub only: other buttons' drags (context menu, canvas pan) must neither write nor commit.
        let drag_started = response.left.drag.started();
        let drag_delta = response.left.drag.delta();
        let drag_stopped = response.left.drag.stopped();
        if drag_started || ui.state::<DragValueState>(id).is_some() {
            ui.with_state::<DragValueState, _>(id, |_, state| {
                match state {
                    DragValueState::Editing {
                        buffer,
                        original,
                        end,
                    } => {
                        if self.editable && !response.disabled {
                            if *end == Some(EditEnd::Canceled) {
                                changed = self.value.restore(*original);
                            } else {
                                changed = self.value.parse_from(buffer, rule);
                                committed = true;
                            }
                        }
                        *state = DragValueState::Idle;
                    }
                    DragValueState::Opening => *state = DragValueState::Idle,
                    DragValueState::Idle | DragValueState::Scrubbing(_) => {}
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
            let travel = AxisKeys::SPIN_BUTTON.read(&mut self.widget, ui);
            if let Some(moved) = self.commit_travel(travel, rule) {
                changed |= moved;
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
                    end: None,
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
        let style = self.style.unwrap_or(&theme.drag_value);
        let plan = style.plan(&response, (), theme.text);
        let caret_width = style.caret_width;
        let look = self.widget.wear(ui, plan);
        if self.editable {
            let padding = text_layout::label_padding(
                self.widget.authored_padding().unwrap(),
                caret_width,
                self.text_align.halign(),
            );
            self.widget.configure().padding(padding);
        }

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
        // The editor wears the chip's looks and box, or the field resizes on click; an unstyled
        // `TextEdit` would wear `theme.text_edit`.
        let editor = self.style.unwrap_or(&ui.theme().drag_value).editor();
        // Hold the editor at the chip's last width: as a `Scroll` field it reports zero content width, so a plain cap would collapse to `min_size`. `Fixed` (floored at `min_size.w`) makes long values scroll. Before the first chip frame, use the field's own sizing.
        let min_size = self.widget.authored_min_size().unwrap_or(Size::ZERO);
        let sizes = self.widget.authored_size().unwrap_or_default();
        let held_w = prev_rect.map(|r| Sizing::fixed(r.size.w.max(min_size.w)));
        let width = held_w.unwrap_or_else(|| sizes.w());
        let (mut buffer, original) =
            match ui.with_state::<DragValueState, _>(id, |_, s| mem::take(s)) {
                DragValueState::Editing {
                    buffer, original, ..
                } => (buffer, original),
                DragValueState::Idle | DragValueState::Scrubbing(_) | DragValueState::Opening => {
                    (self.value.edit_string(), self.value.stored())
                }
            };
        let ended = {
            let edit = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(self.text_align)
                .select_all_on_focus(true)
                .style(&editor)
                .padding(
                    self.widget
                        .authored_padding()
                        .unwrap_or(editor.defaults.padding),
                )
                .size((width, sizes.h()))
                .min_size(min_size)
                .max_size(self.widget.authored_max_size().unwrap_or(Size::INF));
            edit.adopt_placement(&self.widget).show(ui).ended
        };
        let changed = if ended == Some(EditEnd::Canceled) {
            self.value.restore(original)
        } else {
            self.value.parse_from(&buffer, self.rule())
        };
        ui.with_state::<DragValueState, _>(id, |_, s| {
            *s = if ended.is_some() {
                DragValueState::Idle
            } else {
                DragValueState::Editing {
                    buffer,
                    original,
                    end: None,
                }
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

impl DragValue<'_> {
    /// How every write of this drag value stores the number.
    const fn rule(&self) -> StoreRule {
        StoreRule::new(self.min, self.max, self.decimals).step(self.step)
    }

    /// Store where `travel` sends the value, answering whether it changed, or `None` when the
    /// keys asked for nothing: no travel, or an end on an unbounded side. The steps add through
    /// [`BoundNum::commit_drag`], so an integer moves by whole steps from its exact value.
    fn commit_travel(&mut self, travel: Travel, rule: StoreRule) -> Option<bool> {
        let end = |bound: f64| bound.is_finite().then_some(bound);
        match travel.end {
            Some(AxisEnd::Start) => end(self.min).map(|min| self.value.commit_value(min, rule)),
            Some(AxisEnd::End) => end(self.max).map(|max| self.value.commit_value(max, rule)),
            None if travel.is_still() => None,
            None => {
                let grain = self.value.unit(self.decimals);
                let unit = self.step.map_or(grain, |step| step.max(grain));
                let steps = travel.steps + travel.pages * axis_keys::COARSE;
                let by = f64::from(steps) * unit;
                Some(self.value.commit_drag(self.value.read(), by, rule))
            }
        }
    }
}

/// Driving a drag value's editor from outside, by its id, under GTK4's names.
impl DragValue<'static> {
    /// Open the editor of the drag value `id` and give it the focus, as a click does, for an
    /// entry a key or a menu asks for; its next show seeds the editor from the bound value, all
    /// selected. A drag value that is not [editable](DragValue::editable) or is disabled stays a
    /// chip.
    pub fn start_editing(ui: &mut Ui, id: WidgetId) {
        ui.with_state::<DragValueState, _>(id, |_, state| {
            if !matches!(state, DragValueState::Editing { end: None, .. }) {
                *state = DragValueState::Opening;
            }
        });
        ui.set_focus(id);
    }

    /// End the edit of the drag value `id` as `end` says: [`EditEnd::Submitted`] commits the
    /// draft, as Enter does, and [`EditEnd::Canceled`] restores the value the edit opened on, as
    /// Escape does. The drag value's next show resolves it. No-op for one that does not edit.
    pub fn stop_editing(ui: &mut Ui, id: WidgetId, end: EditEnd) {
        if !Self::is_editing(ui, id) {
            return;
        }
        ui.with_state::<DragValueState, _>(id, |_, state| {
            if let DragValueState::Editing { end: ended, .. } = state {
                *ended = Some(end);
            } else {
                *state = DragValueState::Idle;
            }
        });
        if ui.focus() == Some(id) {
            ui.clear_focus();
        }
    }

    /// Whether the drag value `id` shows its editor, or opens it on its next show. Allocates no
    /// row.
    pub fn is_editing(ui: &Ui, id: WidgetId) -> bool {
        matches!(
            ui.state::<DragValueState>(id),
            Some(DragValueState::Opening | DragValueState::Editing { end: None, .. })
        )
    }
}

impl Configure for DragValue<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// Whether a character was typed into the focused chip this frame; a space is its own class, as
/// a spin button doesn't type it.
fn typed(ui: &Ui) -> bool {
    ui.keyboard_events()
        .iter()
        .any(|press| press.class() == KeyClass::Text && !press.text.is_empty())
}

/// An interned suffix's characters copied out of the arena for formatting; retained, so a steady suffix allocates once.
#[derive(Debug, Default)]
struct SuffixScratch(String);

#[cfg(test)]
mod tests;
