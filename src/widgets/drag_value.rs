use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
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
    /// `i64::MIN`/`MAX`, so an unbounded integer clamp is a no-op.
    fn commit_drag(&mut self, raw: f64, decimals: usize, min: f64, max: f64) {
        let (lo, hi) = (min.min(max), min.max(max));
        match self {
            DragNum::I64(v) => **v = (raw.round() as i64).clamp(lo as i64, hi as i64),
            DragNum::F64(v) => **v = round_to_decimals(raw, decimals).clamp(lo, hi),
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

    /// Parse `text` and write it clamped into `[min, max]`, leaving the value
    /// untouched when the text doesn't parse (so partial input like `"3."`
    /// doesn't clobber it). Keyboard entry keeps full precision — only drags
    /// snap to `decimals`.
    fn parse_from(&mut self, text: &str, min: f64, max: f64) {
        let (lo, hi) = (min.min(max), min.max(max));
        match self {
            DragNum::I64(v) => {
                if let Ok(n) = text.parse::<i64>() {
                    **v = n.clamp(lo as i64, hi as i64);
                }
            }
            DragNum::F64(v) => {
                if let Ok(n) = text.parse::<f64>() {
                    **v = n.clamp(lo, hi);
                }
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

/// Per-id drag state captured when a drag latches: the base `value` (so
/// cumulative `drag_delta` offsets from a stable base rather than
/// accumulating frame-to-frame rounding) and the `speed` sampled at that
/// instant. Latching the speed lets a caller pass a value-derived speed
/// (e.g. `max(|v|, 1) * factor` for a magnitude-relative feel) without it
/// shifting mid-drag as the value changes.
#[derive(Debug, Default, Clone, Copy)]
struct DragAnchor {
    value: f64,
    speed: f64,
}

/// Text buffer for a [`DragValue`] in edit mode (see [`DragValue::editable`]).
/// Seeded from the value when a click focuses the field, edited by the inner
/// `TextEdit`, and parsed back into the value each frame while focused.
#[derive(Debug, Default, Clone)]
struct DragEdit {
    buffer: String,
}

/// A numeric field you scrub by dragging horizontally (Blender / egui
/// style): each pixel of horizontal travel changes the value by `speed`,
/// optionally clamped to a range. Binds either an `i64` or an `f64` (see
/// [`DragNum`]) — the integer target rounds to the nearest whole step and a
/// float drag snaps to `decimals`. Renders as a button-styled chip (theme
/// slot `drag_value.chip`) with the formatted number centered inside.
///
/// With [`Self::editable`] the widget is a complete numeric editor: a plain
/// click (no drag) focuses it and swaps the chip for an inline `TextEdit`
/// (theme slot `drag_value.editor`, same box as the chip) for exact keyboard
/// entry; Enter or clicking away commits and returns to the scrub chip. The
/// editor holds the chip's width and **scrolls** a longer full-precision value
/// inside it, so it stays put even in a content-hugging parent.
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

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &self.element);

        // Focused + editable: the inline text editor owns the frame. Pass the
        // chip's last *pre-transform* rect (logical px, matching min/max_size)
        // so the editor holds that width instead of growing a content-hugging
        // parent to fit the full-precision value — `rect` is post-zoom and
        // would mismatch the sizing units under a scaled canvas.
        if self.editable && ui.focused_id() == Some(id) {
            return self.show_editing(ui, id, raw_state.layout_rect);
        }

        let mut element = self.element;

        // Capture the value + speed when the drag latches, then offset by the
        // cumulative travel each frame and commit (snap / round / clamp).
        if state.drag_started() {
            *ui.state_mut::<DragAnchor>(id) = DragAnchor {
                value: self.value.get(),
                speed: self.speed,
            };
        }
        if !state.disabled
            && state.dragged()
            && let Some(delta) = state.drag_delta()
        {
            let anchor = *ui.state_mut::<DragAnchor>(id);
            let raw = anchor.value + delta.x as f64 * anchor.speed;
            self.value
                .commit_drag(raw, self.decimals, self.min, self.max);
        }

        // A plain click (no drag latched) enters keyboard entry. Seed the
        // buffer from the value now so the field shows it the instant it
        // focuses next frame; the editor's `select_all_on_focus` then selects
        // it so the first keystroke replaces it (egui-style click-to-edit).
        if self.editable && state.clicked {
            ui.state_mut::<DragEdit>(id).buffer = self.value.edit_string();
            ui.request_focus(Some(id));
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
        Response::eager(id, ui, raw_state)
    }

    /// Edit mode: render the inline `TextEdit` over the same `id`, centered and
    /// same-styled as the chip (its box matches by theme, not by measuring the
    /// chip), parse the buffer back into the value each frame, and blur on
    /// Enter (Escape / click-away blur themselves).
    fn show_editing(mut self, ui: &mut Ui, id: WidgetId, prev_rect: Option<Rect>) -> Response<'_> {
        let editor = self
            .style
            .take()
            .map(|s| s.editor)
            .unwrap_or_else(|| ui.theme.drag_value.editor.clone());
        // Hold the editor at the width the chip occupied last frame. The chip
        // shows `decimals`-rounded text; the editor shows every digit, which in
        // a content-hugging parent would otherwise grow the field. Capping the
        // width scrolls a long value inside it instead. Floored at `min_size.w`
        // so the cap can't fall below the floor (which would make
        // `resolve_axis_size`'s `clamp(min, max)` panic).
        let max_w = prev_rect
            .map_or(self.element.max_size.w, |r| r.size.w)
            .max(self.element.min_size.w);
        let mut buffer = std::mem::take(&mut ui.state_mut::<DragEdit>(id).buffer);
        let submitted = {
            let resp = TextEdit::new(&mut buffer)
                .id(id)
                .text_align(Align::CENTER)
                .style(editor)
                .select_all_on_focus()
                .size(self.element.size)
                .min_size(self.element.min_size)
                .max_size((max_w, self.element.max_size.h))
                .show(ui);
            resp.submitted
        };
        self.value.parse_from(&buffer, self.min, self.max);
        ui.state_mut::<DragEdit>(id).buffer = buffer;
        if submitted {
            ui.request_focus(None);
        }
        Response::lazy(id, ui)
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
mod tests {
    use crate::widgets::drag_value::{DragEdit, DragNum, round_to_decimals};

    #[test]
    fn round_to_decimals_snaps_and_formats_short() {
        // The reported long value snaps to its 3-decimal display and prints
        // without a tail — that's the whole point (edit_string shows this).
        let r = round_to_decimals(1.984_573_845_634_985_2, 3);
        assert_eq!(r, 1.985);
        assert_eq!(format!("{r:?}"), "1.985");
        // Fewer / zero decimals.
        assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 2), 1.98);
        assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 0), 2.0);
        // Classic float-noise inputs collapse to a clean short value.
        assert_eq!(format!("{:?}", round_to_decimals(0.1 + 0.2, 1)), "0.3");
        assert_eq!(round_to_decimals(12.3456, 2), 12.35);
        // Negative values keep their sign.
        assert_eq!(round_to_decimals(-1.6789, 1), -1.7);
    }

    #[test]
    fn commit_drag_snaps_rounds_and_clamps() {
        const INF: f64 = f64::INFINITY;
        // Float: snaps to `decimals`, unbounded is a no-op clamp.
        let mut f = 0.0;
        DragNum::from(&mut f).commit_drag(1.984_573_845_634_985_2, 3, -INF, INF);
        assert_eq!(f, 1.985);
        // Float: clamps into the range.
        let mut f = 0.0;
        DragNum::from(&mut f).commit_drag(50.0, 2, 0.0, 10.0);
        assert_eq!(f, 10.0);
        // Int: rounds to whole (decimals ignored), unbounded no-op clamp.
        let mut i = 0;
        DragNum::from(&mut i).commit_drag(7.6, 3, -INF, INF);
        assert_eq!(i, 8);
        // Int: clamps into the range.
        let mut i = 0;
        DragNum::from(&mut i).commit_drag(500.0, 0, 0.0, 100.0);
        assert_eq!(i, 100);
    }

    #[test]
    fn drag_num_get_reads_both_variants() {
        let mut f = 2.5_f64;
        assert_eq!(DragNum::from(&mut f).get(), 2.5);
        let mut i = 5_i64;
        assert_eq!(DragNum::from(&mut i).get(), 5.0);
    }

    #[test]
    fn drag_num_edit_string_and_parse_round_trip() {
        const INF: f64 = f64::INFINITY;
        // Float keeps a trailing `.0` so it re-reads as a float, and a
        // fractional value survives verbatim.
        let mut f = 3.0_f64;
        assert_eq!(DragNum::from(&mut f).edit_string(), "3.0");
        let mut f = 2.5_f64;
        let s = DragNum::from(&mut f).edit_string();
        DragNum::from(&mut f).parse_from(&s, -INF, INF);
        assert_eq!(f, 2.5);

        // Int formats and parses back exactly.
        let mut i = -42_i64;
        assert_eq!(DragNum::from(&mut i).edit_string(), "-42");

        // Unparseable text leaves the value untouched (partial input).
        let mut i = 9_i64;
        DragNum::from(&mut i).parse_from("12x", -INF, INF);
        assert_eq!(i, 9);
        DragNum::from(&mut i).parse_from("15", -INF, INF);
        assert_eq!(i, 15);

        // Typed entry clamps into the range too.
        let mut i = 0_i64;
        DragNum::from(&mut i).parse_from("500", 0.0, 100.0);
        assert_eq!(i, 100);
        let mut f = 0.0_f64;
        DragNum::from(&mut f).parse_from("-3.5", 0.0, 1.0);
        assert_eq!(f, 0.0);
    }

    #[test]
    fn editing_a_long_value_holds_the_field_width() {
        use super::DragValue;
        use crate::Ui;
        use crate::forest::Layer;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::primitives::widget_id::WidgetId;
        use crate::widgets::panel::Panel;
        use glam::UVec2;

        let surface = UVec2::new(400, 120);
        let id = WidgetId::from_hash("dv-width");
        let mut v = 1.984_573_845_634_985_2_f64;

        // A `Hug` row makes the field's own content drive its width — the
        // condition where the width-cap matters. The chip shows "1.985"; the
        // editor shows every digit and must scroll inside the chip's width
        // rather than grow the row.
        let render = |ui: &mut Ui, v: &mut f64| -> crate::forest::tree::NodeId {
            let mut node = None;
            Panel::hstack()
                .id(WidgetId::from_hash("dv-row"))
                .size((Sizing::Hug, Sizing::Hug))
                .show(ui, |ui| {
                    node = Some(
                        DragValue::new(v)
                            .editable(true)
                            .decimals(3)
                            .size((Sizing::Fill(1.0), Sizing::Hug))
                            .min_size((40.0, 0.0))
                            .id(id)
                            .show(ui)
                            .node(),
                    );
                });
            node.unwrap()
        };

        let mut ui = Ui::for_test();
        let mut node = None;
        ui.run_at_acked(surface, |ui| node = Some(render(ui, &mut v)));
        let display_w = ui.layout[Layer::Main].rect[node.unwrap().idx()].size.w;

        // Enter edit mode carrying the full-precision text a click would seed.
        ui.state_mut::<DragEdit>(id).buffer = "1.98457384563498524".to_string();
        ui.request_focus(Some(id));
        ui.run_at_acked(surface, |ui| node = Some(render(ui, &mut v)));
        let edit_w = ui.layout[Layer::Main].rect[node.unwrap().idx()].size.w;

        assert!(display_w >= 40.0, "min_size floor honored ({display_w})");
        assert_eq!(
            display_w, edit_w,
            "editing the full-precision value must not resize the field \
             (display {display_w}, edit {edit_w})"
        );
    }

    #[test]
    fn editing_under_a_scaled_canvas_does_not_panic() {
        use super::DragValue;
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::primitives::transform::TranslateScale;
        use crate::primitives::widget_id::WidgetId;
        use crate::widgets::panel::Panel;
        use glam::{UVec2, Vec2};

        let surface = UVec2::new(400, 120);
        let id = WidgetId::from_hash("dv-zoom");
        let mut v = 1.984_573_845_634_985_2_f64;

        // A scaled parent (0.5×) halves the chip's post-transform rect to ~60px
        // while `min_size` is 100 — the cap must read the pre-transform
        // (logical, 120) width and floor at `min_size`, else feeding the 60px
        // post-transform width makes `resolve_axis_size`'s `clamp(100, 60)`
        // panic.
        let mut ui = Ui::for_test();
        let draw = |ui: &mut Ui, v: &mut f64| {
            Panel::zstack()
                .id(WidgetId::from_hash("dv-zoom-row"))
                .transform(TranslateScale::new(Vec2::ZERO, 0.5))
                .size((Sizing::Fixed(120.0), Sizing::Fixed(60.0)))
                .show(ui, |ui| {
                    DragValue::new(v)
                        .editable(true)
                        .decimals(3)
                        .size((Sizing::Fill(1.0), Sizing::Hug))
                        .min_size((100.0, 0.0))
                        .id(id)
                        .show(ui);
                });
        };
        ui.run_at_acked(surface, |ui| draw(ui, &mut v));
        ui.state_mut::<DragEdit>(id).buffer = "1.98457384563498524".to_string();
        ui.request_focus(Some(id));
        ui.run_at_acked(surface, |ui| draw(ui, &mut v));
    }
}
