use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::theme::splitter::SplitterTheme;
use crate::widgets::{Response, WidgetEntry, enter_widget};
use crate::window::CursorIcon;
use glam::Vec2;

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
/// Layout reserves only the visible `rule_thickness` seam between the
/// panes — the wide grab target is an *overlay* bar straddling the seam
/// (sash-style), so no backdrop stripe ever shows through at rest and
/// the hover/drag fill paints over the pane edges it covers.
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
        // Canvas root: the split stack fills it, and the overlay bar is
        // placed at an explicit position on top. Clipped so the bar's
        // overhang at a ratio stop can't paint or hit outside the widget.
        let mut element = Element::new(LayoutMode::Canvas);
        element.size = (Sizing::FILL, Sizing::FILL).into();
        element.flags.set_clip(ClipMode::Rect);
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
            if divider.left.drag.dragging()
                && let (Some(local), Some(rect)) = (state.pointer_local, state.rect)
            {
                let (pos, extent) = if self.horizontal {
                    (local.x, rect.size.w)
                } else {
                    (local.y, rect.size.h)
                };
                ratio = pointer_to_ratio(pos, extent, rule_thickness, self.min_pane);
            }
            if divider.left.double_clicked() {
                ratio = 0.5;
            }
        }
        *self.ratio = ratio;

        let bar_fill = if divider.left.drag.dragging() {
            Some(drag_color)
        } else if divider.hovered && !state.disabled {
            Some(hover_color)
        } else {
            None
        };
        // Resize cursor while the divider is hot. Keyed off `dragged`
        // first: mid-drag the pointer routinely leaves the thin bar
        // (`hovered` is also capture-gated), and the cursor must hold
        // until release.
        if bar_fill.is_some() {
            ui.set_cursor(if self.horizontal {
                CursorIcon::EwResize
            } else {
                CursorIcon::NsResize
            });
        }
        let bar_bg = bar_fill.map(Background::fill).unwrap_or_default();
        let rule_bg = Background::fill(rule_color);

        // The seam's center, from last frame's arranged extent — the
        // overlay bar trails a container resize by one frame (the same
        // lag the drag mapping rides); at rest the bar paints nothing,
        // so the lag can't be seen. First frame: no rect yet, the bar
        // lands off-origin and corrects itself next frame.
        let extent = state
            .rect
            .map(|r| if self.horizontal { r.size.w } else { r.size.h })
            .unwrap_or(0.0);
        let seam = ratio * (extent - rule_thickness).max(0.0) + rule_thickness * 0.5;

        let horizontal = self.horizontal;
        ui.node(id, self.element, None, |ui| {
            // The split stack: panes touching a Fixed(rule) seam —
            // layout reserves only the visible rule, so no backdrop
            // stripe shows between the panes.
            let stack_id = id.with("stack");
            let mut stack = Element::new(if horizontal {
                LayoutMode::HStack
            } else {
                LayoutMode::VStack
            });
            stack.salt = Salt::Verbatim(stack_id);
            stack.size = (Sizing::FILL, Sizing::FILL).into();
            ui.node(stack_id, stack, None, |ui| {
                pane(
                    ui,
                    id.with("first"),
                    horizontal,
                    Sizing::Fill(ratio),
                    |ui| body(ui, SplitHalf::First),
                );

                let rule_id = id.with("rule");
                let mut rule = Element::new(LayoutMode::Leaf);
                rule.salt = Salt::Verbatim(rule_id);
                rule.size = if horizontal {
                    (Sizing::Fixed(rule_thickness), Sizing::FILL)
                } else {
                    (Sizing::FILL, Sizing::Fixed(rule_thickness))
                }
                .into();
                ui.node(rule_id, rule, Some(&rule_bg), |_| {});

                pane(
                    ui,
                    id.with("second"),
                    horizontal,
                    Sizing::Fill(1.0 - ratio),
                    |ui| body(ui, SplitHalf::Second),
                );
            });

            // The grab target: an overlay bar straddling the seam,
            // recorded after the stack so it paints (hover/drag fill)
            // and hit-tests above the pane edges it covers.
            let mut bar = Element::new(LayoutMode::Leaf);
            bar.salt = Salt::Verbatim(divider_id);
            bar.flags.set_sense(Sense::DRAG);
            if horizontal {
                bar.size = (Sizing::Fixed(thickness), Sizing::FILL).into();
                bar.position = Vec2::new(seam - thickness * 0.5, 0.0);
            } else {
                bar.size = (Sizing::FILL, Sizing::Fixed(thickness)).into();
                bar.position = Vec2::new(0.0, seam - thickness * 0.5);
            }
            ui.node(divider_id, bar, Some(&bar_bg), |_| {});
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
/// first pane's share of the free space (`extent − reserved`, where
/// `reserved` is the seam the rule occupies in layout). The seam center
/// follows the pointer; `min_pane` floors both panes, collapsing to a
/// centered clamp when the free space can't fit two floors. Degenerate
/// extents pin to `0.5`.
fn pointer_to_ratio(pos: f32, extent: f32, reserved: f32, min_pane: f32) -> f32 {
    let span = extent - reserved;
    if span <= f32::EPSILON {
        return 0.5;
    }
    // `lo <= span/2 <= hi` by construction, so the clamp can't invert
    // even when `2 * min_pane > span`.
    let lo = min_pane.min(span * 0.5);
    let hi = (span - min_pane).max(span * 0.5);
    (pos - reserved * 0.5).clamp(lo, hi) / span
}

#[cfg(test)]
mod tests;
