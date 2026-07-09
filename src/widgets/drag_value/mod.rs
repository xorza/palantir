use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::{Response, WidgetEntry, button_look, enter_widget};
use std::ops::RangeInclusive;

/// The numeric target a [`DragValue`] scrubs: either an `i64` or an `f64`,
/// borrowed mutably for the widget's lifetime. Build one implicitly through
/// `From` — `DragValue::new(&mut my_i64)` and `DragValue::new(&mut my_f64)`
/// both work. Scrub math runs in `f64`; the integer case rounds back to the
/// nearest whole step on write.
#[derive(Debug)]
pub enum DragNum<'a> {
    I64(&'a mut i64),
    F64(&'a mut f64),
}

impl DragNum<'_> {
    /// The bound value widened to `f64` — captured as the drag anchor.
    fn get(&self) -> f64 {
        match self {
            DragNum::I64(v) => **v as f64,
            DragNum::F64(v) => **v,
        }
    }

    /// Commit a scrubbed `raw` value: the float target snaps to `decimals`
    /// (so a drag never stores a long tail — `1.98457…` at 3 → `1.985`), the
    /// integer target rounds to the nearest whole step; both clamp into
    /// `[min, max]` (a reversed pair is tolerated). Infinite bounds cast to
    /// `i64::MIN`/`MAX`, so an unbounded integer clamp is a no-op. Returns
    /// whether the stored value actually changed — exact for the integer,
    /// bit-exact for the float.
    fn commit_drag(&mut self, raw: f64, decimals: usize, min: f64, max: f64) -> bool {
        let (lo, hi) = (min.min(max), min.max(max));
        match self {
            DragNum::I64(v) => {
                let next = (raw.round() as i64).clamp(lo as i64, hi as i64);
                let changed = **v != next;
                **v = next;
                changed
            }
            DragNum::F64(v) => {
                // `+ 0.0` normalizes -0.0 to +0.0 (IEEE: -0.0 + 0.0 = +0.0):
                // rounding a small negative raw yields -0.0, and clamp's `<`
                // lets it slip through a +0.0 lower bound — the sign would
                // leak into the display ("-0.00") and serialized values.
                let next = round_to_decimals(raw, decimals).clamp(lo, hi) + 0.0;
                let changed = (**v).to_bits() != next.to_bits();
                **v = next;
                changed
            }
        }
    }

    /// Exact, full-precision text for the edit buffer — `{:?}` on the float
    /// keeps a trailing `.0` so a whole value still reads as a float.
    fn edit_string(&self) -> String {
        match self {
            DragNum::I64(v) => v.to_string(),
            DragNum::F64(v) => format!("{:?}", **v),
        }
    }

    /// Parse `text` and write it clamped into `[min, max]`, leaving the
    /// value untouched when the text doesn't parse (partial input like
    /// `"3."`) or parses non-finite — a committed NaN survives clamp and
    /// poisons every subsequent scrub, so `"nan"`/`"inf"` are rejected.
    /// Returns whether the stored value changed. Keyboard entry keeps full
    /// precision — only drags snap to `decimals`.
    fn parse_from(&mut self, text: &str, min: f64, max: f64) -> bool {
        let (lo, hi) = (min.min(max), min.max(max));
        match self {
            DragNum::I64(v) => {
                let Ok(n) = text.parse::<i64>() else {
                    return false;
                };
                let next = n.clamp(lo as i64, hi as i64);
                let changed = **v != next;
                **v = next;
                changed
            }
            DragNum::F64(v) => {
                let Ok(n) = text.parse::<f64>() else {
                    return false;
                };
                if !n.is_finite() {
                    return false;
                }
                let next = n.clamp(lo, hi) + 0.0;
                let changed = (**v).to_bits() != next.to_bits();
                **v = next;
                changed
            }
        }
    }
}

impl<'a> From<&'a mut i64> for DragNum<'a> {
    fn from(v: &'a mut i64) -> Self {
        DragNum::I64(v)
    }
}

impl<'a> From<&'a mut f64> for DragNum<'a> {
    fn from(v: &'a mut f64) -> Self {
        DragNum::F64(v)
    }
}

/// Per-id scrub state captured when a left drag latches on the chip: the
/// base `value` (so cumulative `drag_delta` offsets from a stable base
/// rather than accumulating frame-to-frame rounding), the `speed` sampled
/// at that instant (a value-derived speed must not shift mid-drag), and
/// `last`, the most recent scrubbed result — the stop edge arrives after
/// the drag state is gone, so the commit re-writes `last`. `armed` marks
/// that the anchor belongs to the current gesture; a drag latched while
/// the inline editor owned the id (text selection) never arms and must
/// not scrub.
#[derive(Debug, Default, Clone, Copy)]
struct DragAnchor {
    value: f64,
    speed: f64,
    last: f64,
    armed: bool,
}

/// Text buffer for a [`DragValue`] in edit mode (see [`DragValue::editable`]).
/// Seeded from the value when the editor is entered, edited by the inner
/// `TextEdit`, and parsed back into the value each frame while focused.
/// `editing` marks a pending draft: set while the editor owns the frame,
/// resolved (committed or discarded) by the first chip frame after focus
/// leaves — the chip never renders the inner `TextEdit`, so its
/// `lost_focus` signal can't be observed there.
#[derive(Debug, Default, Clone)]
struct DragEdit {
    buffer: String,
    editing: bool,
}

/// What [`DragValue::show`] returns: the widget's [`Response`] (reachable
/// via `Deref`) plus the edit signals computed inside `show()`, mirroring
/// [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub struct DragValueResponse<'a> {
    /// The widget's pointer/click/hover [`Response`]. Also reachable through
    /// `Deref`, so `resp.clicked()` resolves here.
    pub response: Response<'a>,
    /// The bound value was written with a value differing from what the
    /// caller passed in this frame. Under the commit-deferring pattern
    /// (re-seed from canonical every frame) this is a **level** — true on
    /// every frame an uncommitted draft exists — not a per-input edge.
    /// Live-preview callers apply the value on this.
    pub changed: bool,
    /// A gesture finished this frame and the bound value holds its final
    /// result: the scrub drag released, or edit mode ended (Enter / focus
    /// lost). Callers that treat one gesture as one undoable edit act on
    /// this instead of `changed`.
    pub committed: bool,
}

impl<'a> std::ops::Deref for DragValueResponse<'a> {
    type Target = Response<'a>;
    fn deref(&self) -> &Self::Target {
        &self.response
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
/// entry; Enter, Escape, or clicking away commits and returns to the scrub
/// chip. The editor holds the chip's width and **scrolls** a longer
/// full-precision value inside it, so it stays put even in a
/// content-hugging parent.
///
/// The value is written live — every scrub step and edit-mode reparse lands
/// in the bound target — and [`DragValueResponse`] reports both grains:
/// `changed` per differing write, `committed` once per finished gesture
/// (drag release, Enter, blur). An undo-aware caller can ignore `changed`,
/// re-seed the bound value from its canonical source every frame, and apply
/// it only on `committed`: the widget re-writes the gesture's final value on
/// the commit frame, so the deferred caller still observes it. A gesture
/// that ends while the widget is disabled (or, for a pending edit, no
/// longer editable) is dropped, not committed.
pub struct DragValue<'a> {
    element: Element,
    value: DragNum<'a>,
    speed: f64,
    min: f64,
    max: f64,
    decimals: usize,
    suffix: &'static str,
    editable: bool,
    style: Option<DragValueTheme>,
}

impl<'a> DragValue<'a> {
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.flags.set_sense(Sense::DRAG);
        Self {
            element,
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
        if on {
            self.element.flags.set_sense(Sense::CLICK | Sense::DRAG);
        }
        self
    }

    /// Override the theme for both the chip and the inline editor. `None`
    /// (default) inherits [`crate::Theme::drag_value`].
    pub fn style(mut self, s: DragValueTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> DragValueResponse<'_> {
        let WidgetEntry {
            id,
            raw: mut raw_state,
            merged: state,
        } = enter_widget(ui, &self.element);

        // Focused + editable + enabled: the inline text editor owns the
        // frame. Pass the chip's last *pre-transform* rect (logical px,
        // matching min/max_size) so the editor holds that width instead of
        // growing a content-hugging parent to fit the full-precision value —
        // `rect` is post-zoom and would mismatch the sizing units under a
        // scaled canvas. Disabled mid-edit falls through to the chip path,
        // which kicks focus out and discards the pending draft below.
        if self.editable && ui.focused_id() == Some(id) {
            if state.disabled {
                ui.request_focus(None);
            } else {
                return self.show_editing(ui, id, raw_state.layout_rect);
            }
        }

        let mut element = self.element;
        let mut changed = false;
        let mut committed = false;

        // A pending edit draft is resolved on the first chip frame after the
        // editor (Escape / click-away — Enter commits inside show_editing):
        // committed while the widget is still an enabled editor, otherwise
        // discarded. The flag clears either way, so a draft stranded by a
        // mid-edit `.editable(false)` or disable can never replay later as a
        // phantom commit. The probe is read-only — idle chips get no state
        // row.
        if ui.try_state::<DragEdit>(id).is_some_and(|e| e.editing) {
            let edit = ui.state_mut::<DragEdit>(id);
            edit.editing = false;
            if self.editable && !state.disabled {
                changed = self.value.parse_from(&edit.buffer, self.min, self.max);
                committed = true;
            }
        }

        // Left-button scrub only — a right/middle drag is someone else's
        // gesture (context menu, canvas pan) and must neither write nor
        // commit. Capture the value + speed when the drag latches, then
        // offset by the cumulative travel each frame and commit
        // (snap / round / clamp). The write is gated on `armed`: a drag
        // latched while the editor owned this id (text selection) has no
        // anchor for this gesture.
        if state.drag_started_by(PointerButton::Left) {
            *ui.state_mut::<DragAnchor>(id) = DragAnchor {
                value: self.value.get(),
                speed: self.speed,
                last: self.value.get(),
                armed: true,
            };
        }
        if !state.disabled
            && let Some(delta) = state.drag_delta_by(PointerButton::Left)
        {
            let anchor = ui.state_mut::<DragAnchor>(id);
            if anchor.armed {
                let raw = anchor.value + delta.x as f64 * anchor.speed;
                changed |= self
                    .value
                    .commit_drag(raw, self.decimals, self.min, self.max);
                anchor.last = self.value.get();
            }
        }
        // The stop edge is the commit: the drag state is already gone on
        // this frame, so `anchor.last` carries the final value — a
        // commit-deferring caller re-seeds the stale pre-drag value every
        // frame, including this one. Released while disabled, the gesture
        // is dropped, not committed; disarming either way ends it.
        if state.drag_stopped_by(PointerButton::Left) {
            let anchor = ui.state_mut::<DragAnchor>(id);
            if anchor.armed {
                anchor.armed = false;
                let last = anchor.last;
                if !state.disabled {
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
        if self.editable && !state.disabled && state.clicked {
            ui.request_focus(Some(id));
            // Keep the response's documented focused-synchronicity: the raw
            // snapshot predates the request.
            raw_state.focused = true;
        }

        let text = match &self.value {
            DragNum::I64(v) => ui.fmt(format_args!("{}{}", **v, self.suffix)),
            DragNum::F64(v) => ui.fmt(format_args!("{:.*}{}", self.decimals, **v, self.suffix)),
        };

        let chip = self.style.as_ref().map(|s| &s.chip);
        let look = button_look(ui, id, &mut element, state, chip);

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
                weight: look.text.weight,
            });
        });
        DragValueResponse {
            response: Response::eager(id, ui, raw_state),
            changed,
            committed,
        }
    }

    /// Edit mode: render the inline `TextEdit` over the same `id`, centered
    /// and same-styled as the chip (its box matches by theme, not by
    /// measuring the chip), parse the buffer back into the value each frame,
    /// and blur on Enter (Escape / click-away blur themselves; the chip path
    /// resolves the pending draft next frame via `DragEdit::editing`).
    fn show_editing(
        mut self,
        ui: &mut Ui,
        id: WidgetId,
        prev_rect: Option<Rect>,
    ) -> DragValueResponse<'_> {
        let editor = self
            .style
            .take()
            .map(|s| s.editor)
            .unwrap_or_else(|| ui.theme.drag_value.editor.clone());
        // Hold the editor at exactly the width the chip occupied last frame.
        // The chip shows `decimals`-rounded text; the editor shows every digit
        // and, as a `Scroll` field, reports zero content width — so nothing
        // pulls a `Fill` field up to the chip's width and a plain cap would let
        // it collapse to `min_size`. Pin the width with `Fixed` (floored at
        // `min_size.w`) so a long value scrolls inside the chip's box instead
        // of growing a content-hugging row. Before the first chip frame gives
        // us a width to hold, fall back to the field's own width sizing.
        let held_w = prev_rect.map(|r| Sizing::Fixed(r.size.w.max(self.element.min_size.w)));
        let width = held_w.unwrap_or(self.element.size.w());
        // Entry edge: seed the buffer from the current value — a click and a
        // programmatic focus both get a fresh draft, never a previous
        // session's stale text — and disarm any scrub anchor, so a drag that
        // survives into (or latches during) edit mode can't later commit as
        // a scrub over the typed value.
        let edit = ui.state_mut::<DragEdit>(id);
        let entering = !edit.editing;
        if entering {
            edit.editing = true;
            edit.buffer = self.value.edit_string();
        }
        let mut buffer = std::mem::take(&mut edit.buffer);
        if entering && ui.try_state::<DragAnchor>(id).is_some_and(|a| a.armed) {
            ui.state_mut::<DragAnchor>(id).armed = false;
        }
        let submitted = {
            let resp = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(Align::CENTER)
                .style(editor)
                .select_all_on_focus()
                .size((width, self.element.size.h()))
                .min_size(self.element.min_size)
                .max_size(self.element.max_size)
                .show(ui);
            resp.submitted
        };
        let changed = self.value.parse_from(&buffer, self.min, self.max);
        let edit = ui.state_mut::<DragEdit>(id);
        edit.buffer = buffer;
        let mut committed = false;
        if submitted {
            edit.editing = false;
            committed = true;
            ui.request_focus(None);
        }
        DragValueResponse {
            response: Response::lazy(id, ui),
            changed,
            committed,
        }
    }
}

impl Configure for DragValue<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// Round `v` to `decimals` fractional digits. Shifts by `10^decimals`,
/// rounds, and divides back — the divide-by-a-power-of-ten (rather than a
/// multiply by `10^-decimals`) lands on the nearest f64 to a short decimal,
/// so the result formats without a long tail (1.98457… at 3 → 1.985).
fn round_to_decimals(v: f64, decimals: usize) -> f64 {
    // `10^decimals` overflows to `inf` past ~308 digits (and `f64` carries no
    // more than ~15 anyway); clamp so the shift stays finite and the fn total.
    let p = 10f64.powi(decimals.min(15) as i32);
    (v * p).round() / p
}

#[cfg(test)]
mod tests;
