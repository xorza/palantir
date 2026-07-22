use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::{Configure, ConfigureElement, Element, Salt};
use crate::ui::Ui;
use crate::widgets::theme::splitter::SplitterTheme;
use crate::widgets::{Response, enter_widget};
use crate::window::CursorIcon;

/// Two panes split by a draggable divider. [`Splitter::horizontal`] lays
/// the panes side by side (vertical divider bar); [`Splitter::vertical`]
/// stacks them (horizontal bar). The caller owns the split as `ratio` —
/// the first pane's share of the free space, `0..1`. While dragging,
/// the current pointer target feeds layout immediately; the widget writes
/// the resulting content-constrained share back on the following record.
/// Double-clicking the divider recenters to `0.5`. Panes clip their content
/// so an oversized body can't bleed across the divider mid-resize. Visuals
/// come from [`crate::SplitterTheme`] (theme slot `splitter`).
///
/// One Grid owns the pane tracks and the visible `rule_thickness` seam.
/// The wide grab target is a late-recorded overlay in the rule's cell,
/// so layout places it at the content-constrained boundary without a
/// second layout pass.
///
/// [`Splitter::show`] records both panes through one `FnMut` body called
/// with [`SplitHalf::First`] then [`SplitHalf::Second`] — one closure, so
/// a recursive pane tree can capture its state mutably once.
#[derive(Debug)]
pub struct Splitter<'a> {
    element: Element,
    ratio: &'a mut f32,
    axis: Axis,
    min_pane: f32,
    style: Option<&'a SplitterTheme>,
}

/// Which pane [`Splitter::show`]'s body is currently recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitHalf {
    First,
    Second,
}

#[derive(Debug, Default)]
struct SplitterState {
    sync_ratio_next_record: bool,
}

impl<'a> Splitter<'a> {
    /// Side-by-side panes with a vertical divider bar; `ratio` is the
    /// left pane's share.
    #[track_caller]
    pub fn horizontal(ratio: &'a mut f32) -> Self {
        Self::new(ratio, Axis::X)
    }

    /// Stacked panes with a horizontal divider bar; `ratio` is the top
    /// pane's share.
    #[track_caller]
    pub fn vertical(ratio: &'a mut f32) -> Self {
        Self::new(ratio, Axis::Y)
    }

    #[track_caller]
    fn new(ratio: &'a mut f32, axis: Axis) -> Self {
        // The clipped root contains the grab overlay's overhang within the splitter.
        let mut element = Element::grid();
        element.size = (Sizing::FILL, Sizing::FILL).into();
        element.flags.set_clip(ClipMode::Rect);
        Self {
            element,
            ratio,
            axis,
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

    /// Borrow a theme override for this splitter. The default inherits
    /// [`crate::Theme::splitter`].
    pub fn style(mut self, s: &'a SplitterTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show<'u>(
        self,
        ui: &'u mut Ui,
        mut body: impl FnMut(&mut Ui, SplitHalf),
    ) -> Response<'u> {
        let entry = enter_widget(ui, &self.element);
        let id = entry.id;
        let state = &entry.state;

        let theme = self.style.unwrap_or(&ui.theme.splitter);
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
        let first_id = id.with("first");
        let second_id = id.with("second");
        let axis = self.axis;

        let sync_pending = ui
            .try_state::<SplitterState>(id)
            .is_some_and(|state| state.sync_ratio_next_record);
        let synced_ratio = if sync_pending {
            arranged_pane_ratio(ui, first_id, second_id, axis)
        } else {
            None
        };
        let ratio = synced_ratio.unwrap_or_else(|| sanitize_ratio(*self.ratio));
        let mut layout_ratio = ratio;
        let mut resizing = false;
        if !state.disabled {
            // Divider follows the pointer: map the container-local
            // position on the split axis to the first pane's share.
            if divider.left.drag.dragging()
                && let (Some(local), Some(rect)) = (state.pointer_local, state.layout_rect)
            {
                layout_ratio = pointer_to_ratio(
                    axis.main_v(local),
                    axis.main(rect.size),
                    rule_thickness,
                    self.min_pane,
                );
                resizing = true;
            }
            if divider.left.double_clicked() {
                layout_ratio = 0.5;
                resizing = true;
            }
        }
        *self.ratio = ratio;

        {
            let grid_state = ui.state_mut::<SplitterState>(id);
            grid_state.sync_ratio_next_record =
                resizing || (sync_pending && synced_ratio.is_none());
        }

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
            ui.set_cursor(match axis {
                Axis::X => CursorIcon::EwResize,
                Axis::Y => CursorIcon::NsResize,
            });
        }
        let bar_bg = bar_fill.map(Background::fill).unwrap_or_default();
        let rule_bg = Background::fill(rule_color);

        let main_tracks = [
            Track::new(Sizing::share(layout_ratio)),
            Track::fixed(rule_thickness),
            Track::new(Sizing::share(1.0 - layout_ratio)),
        ];
        let cross_tracks = [Track::fill()];
        let layer = ui.forest.current_layer();
        let mut element = self.element;
        let grid_def_id = match axis {
            Axis::X => ui.forest.trees[layer].push_grid_def(&cross_tracks, &main_tracks, 0.0, 0.0),
            Axis::Y => ui.forest.trees[layer].push_grid_def(&main_tracks, &cross_tracks, 0.0, 0.0),
        };
        element.set_grid_def(grid_def_id);
        ui.node(id, element, None, |ui| {
            pane(ui, first_id, axis, 0, |ui| body(ui, SplitHalf::First));

            let rule_id = id.with("rule");
            let mut rule = Element::leaf();
            rule.salt = Salt::Verbatim(rule_id);
            rule.size = (Sizing::FILL, Sizing::FILL).into();
            set_main_cell(&mut rule, axis, 1);
            ui.node(rule_id, rule, Some(&rule_bg), |_| {});

            pane(ui, second_id, axis, 2, |ui| body(ui, SplitHalf::Second));

            let inset = (rule_thickness - thickness) * 0.5;
            let mut bar = Element::leaf();
            bar.salt = Salt::Verbatim(divider_id);
            bar.flags.set_sense(Sense::DRAG);
            bar.size = (Sizing::FILL, Sizing::FILL).into();
            bar.margin = match axis {
                Axis::X => (inset, 0.0, inset, 0.0).into(),
                Axis::Y => (0.0, inset, 0.0, inset).into(),
            };
            set_main_cell(&mut bar, axis, 1);
            ui.node(divider_id, bar, Some(&bar_bg), |_| {});
        });

        entry.into_response(ui)
    }
}

impl Configure for Splitter<'_> {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
    }
}

/// One pane: a clipped ZStack filling its Grid cell.
fn pane(ui: &mut Ui, id: WidgetId, axis: Axis, main_cell: u16, body: impl FnOnce(&mut Ui)) {
    let mut el = Element::zstack();
    el.salt = Salt::Verbatim(id);
    el.size = (Sizing::FILL, Sizing::FILL).into();
    el.flags.set_clip(ClipMode::Rect);
    set_main_cell(&mut el, axis, main_cell);
    ui.node(id, el, None, body)
}

fn set_main_cell(element: &mut Element, axis: Axis, main_cell: u16) {
    match axis {
        Axis::X => element.grid.col = main_cell,
        Axis::Y => element.grid.row = main_cell,
    }
}

/// Recover the first pane's effective share after layout applied both
/// panes' intrinsic content floors. The next record writes this back while
/// the current layout remains free to follow the latest pointer target.
fn arranged_pane_ratio(
    ui: &Ui,
    first_id: WidgetId,
    second_id: WidgetId,
    axis: Axis,
) -> Option<f32> {
    let first = ui.response_for(first_id).layout_rect?;
    let second = ui.response_for(second_id).layout_rect?;
    let first_extent = axis.main(first.size);
    let second_extent = axis.main(second.size);
    let span = first_extent + second_extent;
    (span > f32::EPSILON).then(|| sanitize_ratio(first_extent / span))
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
