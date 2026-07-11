use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::splitter::SplitterTheme;
use crate::widgets::{Response, WidgetEntry, enter_widget};

/// Two panes split by a draggable divider. [`Splitter::horizontal`] lays
/// the panes side by side (vertical divider bar); [`Splitter::vertical`]
/// stacks them (horizontal bar). The caller owns the split as `ratio` —
/// the first pane's share of the free space, `0..1` — and the widget
/// writes it back while the divider is dragged, mapping the pointer
/// through last frame's arranged extent (one-frame lag, invisible at
/// interactive rates — same trick as [`crate::Slider`]). Double-clicking
/// the divider recenters to `0.5`. Panes clip their content so an
/// oversized body can't bleed across the divider mid-resize. Visuals
/// come from [`crate::SplitterTheme`] (theme slot `splitter`).
///
/// [`Splitter::show`] records both panes through one `FnMut` body called
/// with [`SplitHalf::First`] then [`SplitHalf::Second`] — one closure, so
/// a recursive pane tree can capture its state mutably once.
pub struct Splitter<'a> {
    element: Element,
    ratio: &'a mut f32,
    horizontal: bool,
    min_pane: f32,
    style: Option<SplitterTheme>,
}

/// Which pane [`Splitter::show`]'s body is currently recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitHalf {
    First,
    Second,
}

impl<'a> Splitter<'a> {
    /// Side-by-side panes with a vertical divider bar; `ratio` is the
    /// left pane's share.
    #[track_caller]
    pub fn horizontal(ratio: &'a mut f32) -> Self {
        Self::axis(ratio, true)
    }

    /// Stacked panes with a horizontal divider bar; `ratio` is the top
    /// pane's share.
    #[track_caller]
    pub fn vertical(ratio: &'a mut f32) -> Self {
        Self::axis(ratio, false)
    }

    #[track_caller]
    fn axis(ratio: &'a mut f32, horizontal: bool) -> Self {
        let mut element = Element::new(if horizontal {
            LayoutMode::HStack
        } else {
            LayoutMode::VStack
        });
        element.size = (Sizing::FILL, Sizing::FILL).into();
        Self {
            element,
            ratio,
            horizontal,
            min_pane: 0.0,
            style: None,
        }
    }

    /// Floor either pane's split-axis extent at `px` while dragging.
    /// Default `0.0` (panes can collapse to nothing).
    pub fn min_pane(mut self, px: f32) -> Self {
        self.min_pane = px.max(0.0);
        self
    }

    /// Override the theme for this splitter. `None` (default) inherits
    /// [`crate::Theme::splitter`].
    pub fn style(mut self, s: SplitterTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show<'u>(
        self,
        ui: &'u mut Ui,
        mut body: impl FnMut(&mut Ui, SplitHalf),
    ) -> Response<'u> {
        let WidgetEntry {
            id,
            raw,
            merged: state,
        } = enter_widget(ui, &self.element);

        let theme = self.style.as_ref().unwrap_or(&ui.theme.splitter);
        let thickness = theme.thickness.max(1.0);
        let rule_thickness = theme.rule_thickness.max(0.0);
        let rule_color = theme.rule;
        let hover_color = theme.hover;
        let drag_color = theme.drag;

        // The divider's interaction state drives both the ratio write
        // and its own paint. Last frame's response — the recording below
        // is this frame's.
        let divider_id = id.with("divider");
        let divider = ui.response_for(divider_id);

        let mut ratio = sanitize_ratio(*self.ratio);
        if !state.disabled {
            // Divider follows the pointer: map the container-local
            // position on the split axis to the first pane's share.
            if divider.dragged()
                && let (Some(local), Some(rect)) = (state.pointer_local, state.rect)
            {
                let (pos, extent) = if self.horizontal {
                    (local.x, rect.size.w)
                } else {
                    (local.y, rect.size.h)
                };
                ratio = pointer_to_ratio(pos, extent, thickness, self.min_pane);
            }
            if divider.double_clicked() {
                ratio = 0.5;
            }
        }
        *self.ratio = ratio;

        let bar_fill = if divider.dragged() {
            Some(drag_color)
        } else if divider.hovered && !state.disabled {
            Some(hover_color)
        } else {
            None
        };
        let bar_bg = bar_fill.map(Background::fill).unwrap_or_default();
        // Center the resting rule by padding the bar down to its breadth.
        let pad = ((thickness - rule_thickness) * 0.5).max(0.0);
        let rule_bg = Background::fill(rule_color);

        let horizontal = self.horizontal;
        ui.node(id, self.element, None, |ui| {
            pane(
                ui,
                id.with("first"),
                horizontal,
                Sizing::Fill(ratio),
                |ui| body(ui, SplitHalf::First),
            );

            let mut el = Element::new(LayoutMode::ZStack);
            el.salt = Salt::Verbatim(divider_id);
            el.flags.set_sense(Sense::DRAG);
            if horizontal {
                el.size = (Sizing::Fixed(thickness), Sizing::FILL).into();
                el.padding = Spacing::new(pad, 0.0, pad, 0.0);
            } else {
                el.size = (Sizing::FILL, Sizing::Fixed(thickness)).into();
                el.padding = Spacing::new(0.0, pad, 0.0, pad);
            }
            ui.node(divider_id, el, Some(&bar_bg), |ui| {
                let rule_id = divider_id.with("rule");
                let mut rule = Element::new(LayoutMode::Leaf);
                rule.salt = Salt::Verbatim(rule_id);
                rule.size = (Sizing::FILL, Sizing::FILL).into();
                ui.node(rule_id, rule, Some(&rule_bg), |_| {});
            });

            pane(
                ui,
                id.with("second"),
                horizontal,
                Sizing::Fill(1.0 - ratio),
                |ui| body(ui, SplitHalf::Second),
            );
        });

        Response::eager(id, ui, raw)
    }
}

impl Configure for Splitter<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// One pane: a clipped ZStack sized `main` on the split axis, filling
/// the cross axis.
fn pane(ui: &mut Ui, id: WidgetId, horizontal: bool, main: Sizing, body: impl FnOnce(&mut Ui)) {
    let mut el = Element::new(LayoutMode::ZStack);
    el.salt = Salt::Verbatim(id);
    el.size = if horizontal {
        (main, Sizing::FILL).into()
    } else {
        (Sizing::FILL, main).into()
    };
    el.flags.set_clip(ClipMode::Rect);
    ui.node(id, el, None, body)
}

/// A caller-supplied ratio, made safe to use as a `Fill` weight:
/// non-finite pins to center, everything else clamps to `0..1`.
fn sanitize_ratio(r: f32) -> f32 {
    if r.is_finite() {
        r.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Map a container-local pointer coordinate on the split axis to the
/// first pane's share of the free space (`extent − thickness`). The
/// divider center follows the pointer; `min_pane` floors both panes,
/// collapsing to a centered clamp when the free space can't fit two
/// floors. Degenerate extents pin to `0.5`.
fn pointer_to_ratio(pos: f32, extent: f32, thickness: f32, min_pane: f32) -> f32 {
    let span = extent - thickness;
    if span <= f32::EPSILON {
        return 0.5;
    }
    // `lo <= span/2 <= hi` by construction, so the clamp can't invert
    // even when `2 * min_pane > span`.
    let lo = min_pane.min(span * 0.5);
    let hi = (span - min_pane).max(span * 0.5);
    (pos - thickness * 0.5).clamp(lo, hi) / span
}

#[cfg(test)]
mod tests {
    use super::{pointer_to_ratio, sanitize_ratio};

    #[test]
    fn pointer_to_ratio_maps_center_edges_and_floors() {
        // extent 406, thickness 6 → span 400; divider center at
        // pointer, so pointer 203 → first = 200 → ratio 0.5.
        let cases = [
            // (pos, extent, thickness, min_pane, want)
            (203.0, 406.0, 6.0, 0.0, 0.5),
            (3.0, 406.0, 6.0, 0.0, 0.0),   // at the left stop
            (403.0, 406.0, 6.0, 0.0, 1.0), // at the right stop
            (-50.0, 406.0, 6.0, 0.0, 0.0), // past the ends clamps
            (999.0, 406.0, 6.0, 0.0, 1.0),
            (103.0, 406.0, 6.0, 0.0, 0.25),   // quarter point
            (10.0, 406.0, 6.0, 50.0, 0.125),  // min_pane floors first: 50/400
            (395.0, 406.0, 6.0, 50.0, 0.875), // …and second: 350/400
            (7.0, 406.0, 6.0, 300.0, 0.5),    // floors can't both fit → center
            (10.0, 4.0, 6.0, 0.0, 0.5),       // degenerate extent
        ];
        for (pos, extent, thickness, min_pane, want) in cases {
            let got = pointer_to_ratio(pos, extent, thickness, min_pane);
            assert!(
                (got - want).abs() < 1e-6,
                "p2r({pos},{extent},{thickness},{min_pane})={got} want {want}"
            );
        }
    }

    #[test]
    fn sanitize_ratio_clamps_and_pins_non_finite() {
        assert_eq!(sanitize_ratio(0.3), 0.3);
        assert_eq!(sanitize_ratio(-0.2), 0.0);
        assert_eq!(sanitize_ratio(1.5), 1.0);
        assert_eq!(sanitize_ratio(f32::NAN), 0.5);
        assert_eq!(sanitize_ratio(f32::INFINITY), 0.5);
    }
}
