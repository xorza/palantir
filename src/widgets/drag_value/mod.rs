//! The scrubbable number field — drag to change, click to type. Holds the
//! widget, the integer-or-float target it writes through, the retained
//! drag and edit state, and what a frame of either reports.

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::drag_num::DragNum;
use crate::widgets::response::Response;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use crate::widgets::value_response::ValueResponse;
use std::ops::RangeInclusive;

/// One mutually exclusive interaction per [`DragValue`] id. A scrub keeps
/// its sampled base and speed so cumulative pointer travel remains stable;
/// an edit keeps its draft after focus leaves until the chip can resolve it.
#[derive(Debug, Default)]
enum DragValueState {
    #[default]
    Idle,
    Scrubbing {
        value: f64,
        speed: f64,
        /// Last scrubbed result, retained because the stop edge no longer
        /// carries drag distance and deferred callers re-seed the old value.
        last: f64,
    },
    Editing {
        buffer: String,
    },
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
/// entry; Enter, Escape, or clicking away commits and returns to the scrub
/// chip. The editor holds the chip's width and **scrolls** a longer
/// full-precision value inside it, so it stays put even in a
/// content-hugging parent.
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
pub struct DragValue<'a> {
    node: Node,
    value: DragNum<'a>,
    speed: f64,
    min: f64,
    max: f64,
    decimals: usize,
    suffix: &'static str,
    editable: bool,
    style: Option<&'a DragValueTheme>,
}

impl<'a> DragValue<'a> {
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>) -> Self {
        Self {
            node: Node::leaf(),
            value: value.into(),
            speed: 1.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            decimals: 2,
            suffix: "",
            editable: false,
            style: None,
        }
    }

    /// Value change per logical pixel of horizontal drag. Default `1.0`.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Clamp the value into `range`. Default unbounded.
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.min = *range.start();
        self.max = *range.end();
        self
    }

    /// Digits after the decimal point. Governs both the scrub display *and*
    /// the precision a float drag snaps to (so dragging never stores a long
    /// tail — the value matches what's shown). Keyboard entry stays exact.
    /// Ignored by the integer target. Default `2`.
    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Static text appended after the number (e.g. `"px"`, `"%"`).
    pub fn suffix(mut self, s: &'static str) -> Self {
        self.suffix = s;
        self
    }

    /// Enable click-to-type keyboard entry alongside drag-to-scrub. A click
    /// (that doesn't latch a drag) focuses the field and swaps the chip for
    /// an inline `TextEdit`; Enter / click-away commits. Default off.
    pub fn editable(mut self, on: bool) -> Self {
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
        match self.editable {
            true => Sense::CLICK | Sense::DRAG,
            false => Sense::DRAG,
        }
    }

    style_setter!(
        'a,
        DragValueTheme,
        drag_value,
        "Covers both modes at once — the scrub chip and the inline editor.",
    );

    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let sense = self.node.flags.sense() | self.required_sense();
        self.node.flags.set_sense(sense);
        let mut widget = ui.widget(self.node);
        let mut response = widget.response(ui);
        let id = widget.id();

        // Focused + editable + enabled: the inline text editor owns the
        // frame. Pass the chip's last *pre-transform* rect (logical px,
        // matching min/max_size) so the editor holds that width instead of
        // growing a content-hugging parent to fit the full-precision value —
        // `rect` is post-zoom and would mismatch the sizing units under a
        // scaled canvas. Disabled mid-edit falls through to the chip path,
        // which kicks focus out and discards the pending draft below.
        if self.editable && ui.focused_id() == Some(id) {
            if response.disabled {
                ui.request_focus(None);
            } else {
                return self.show_editing(ui, id, response.layout_rect);
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
        let state = if drag_started {
            Some(ui.state_mut::<DragValueState>(id))
        } else {
            ui.try_state_mut::<DragValueState>(id)
        };
        if let Some(state) = state {
            // Escape / click-away reaches the chip with the edit draft still
            // present. Resolve it while editable and enabled, otherwise drop
            // it so a later focus cannot replay stale input.
            if let DragValueState::Editing { buffer } = state {
                if self.editable && !response.disabled {
                    changed = self.value.parse_from(buffer, self.min, self.max);
                    committed = true;
                }
                *state = DragValueState::Idle;
            }

            if drag_started {
                let value = self.value.get();
                *state = DragValueState::Scrubbing {
                    value,
                    speed: self.speed,
                    last: value,
                };
            }

            let mut stopped_at = None;
            if let DragValueState::Scrubbing { value, speed, last } = state {
                if !response.disabled
                    && let Some(delta) = drag_delta
                {
                    let raw = *value + delta.x as f64 * *speed;
                    changed |= self
                        .value
                        .commit_drag(raw, self.decimals, self.min, self.max);
                    *last = self.value.get();
                }
                if drag_stopped {
                    stopped_at = Some(*last);
                }
            }
            // The stop edge is the commit: the drag state is already gone on
            // this frame, so `last` carries the final value. Released while
            // disabled, the gesture is dropped instead.
            if let Some(last) = stopped_at {
                *state = DragValueState::Idle;
                if !response.disabled {
                    changed |= self
                        .value
                        .commit_drag(last, self.decimals, self.min, self.max);
                    committed = true;
                }
            }
        }

        // A plain enabled click (no drag latched) enters keyboard entry;
        // `show_editing` seeds the buffer on entry, so a click and a
        // programmatic `request_focus` get the same fresh draft.
        if self.editable && !response.disabled && response.left.clicked() {
            ui.request_focus(Some(id));
            response.mark_focused();
        }

        let text = match &self.value {
            DragNum::I64(v) => ui.fmt(format_args!("{}{}", **v, self.suffix)),
            DragNum::F64(v) => ui.fmt(format_args!("{:.*}{}", self.decimals, **v, self.suffix)),
        };

        // The chip half of the bundle — the same one the edit mode's editor
        // takes its half from, so the two modes stay in sync under a global
        // restyle.
        let theme = ui.theme();
        let chip = &self.slot(theme).chip;
        let look = chip.plan(&response, (), &theme.text).apply(ui, &mut widget);

        widget.record(ui, Some(&look.background), |ui| {
            ui.add_shape(
                Shape::text(text, look.text.font())
                    .color(look.text.color)
                    .wrap(TextWrap::Truncate)
                    .align(Align::CENTER),
            );
        });
        ValueResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }

    /// Edit mode: render the inline `TextEdit` over the same `id`, centered
    /// and same-styled as the chip (its box matches by theme, not by
    /// measuring the chip), parse the buffer back into the value each frame,
    /// and blur on Enter (Escape / click-away blur themselves; the chip path
    /// resolves the pending [`DragValueState::Editing`] draft).
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
        let ui_theme = ui.theme().clone();
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
        let min_size = self.node.min_size.unwrap_or(Size::ZERO);
        let sizes = self.node.size.unwrap_or_default();
        let held_w = prev_rect.map(|r| Sizing::fixed(r.size.w.max(min_size.w)));
        let width = held_w.unwrap_or(sizes.w());
        // Entry replaces any scrub state atomically, so its later release
        // cannot overwrite the typed result. Existing edit frames move the
        // same String through TextEdit without allocating a new buffer.
        let mut buffer = match std::mem::take(ui.state_mut::<DragValueState>(id)) {
            DragValueState::Editing { buffer } => buffer,
            DragValueState::Idle | DragValueState::Scrubbing { .. } => self.value.edit_string(),
        };
        let submitted = {
            let edit = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(Align::CENTER)
                .select_all_on_focus()
                .style(editor)
                .size((width, sizes.h()))
                .min_size(min_size)
                .max_size(self.node.max_size.unwrap_or(Size::INF));
            // The chip's placement has to survive the swap or the field
            // visibly jumps mid-interaction; which fields that means is
            // `TextEdit`'s call, and documented there.
            let resp = edit.adopt_placement(self.node).show(ui);
            resp.submitted
        };
        let changed = self.value.parse_from(&buffer, self.min, self.max);
        *ui.state_mut::<DragValueState>(id) = if submitted {
            DragValueState::Idle
        } else {
            DragValueState::Editing { buffer }
        };
        if submitted {
            ui.request_focus(None);
        }
        ValueResponse {
            response: Response::lazy(id, ui),
            changed,
            committed: submitted,
        }
    }
}

impl_configure!(DragValue<'_>);

#[cfg(test)]
mod tests;
