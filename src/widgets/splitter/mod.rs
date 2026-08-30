//! Two panes divided by a draggable rule: the widget, the per-pane bodies
//! it takes, and the split ratio it keeps between frames.

use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::approx;
use crate::primitives::background::Background;
use crate::primitives::num::F32Ext;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::splitter::SplitterTheme;
use crate::window::cursor_icon::CursorIcon;

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
/// a recursive pane tree can capture its response mutably once.
#[derive(Debug)]
pub struct Splitter<'a> {
    node: Node,
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
        Self {
            // The clipped root contains the grab overlay's overhang within
            // the splitter.
            node: Node::grid().size((Sizing::FILL, Sizing::FILL)).clip_rect(),
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

    style_setter!('a, SplitterTheme, splitter);

    pub fn show<'u>(
        self,
        ui: &'u mut Ui,
        mut body: impl FnMut(&mut Ui, SplitHalf),
    ) -> Response<'u> {
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        let theme = self.slot(ui.theme());
        let thickness = theme.thickness.max(1.0);
        let rule_thickness = theme.rule_thickness.max(0.0);
        let rule_color = theme.rule;
        let hover_color = theme.hover;
        let drag_color = theme.drag;

        // The divider's interaction response drives both the ratio write
        // and its own paint. Last frame's response — the recording below
        // is this frame's.
        let divider_id = id.with("divider");
        let divider = ui.response_for(divider_id);
        let first_id = id.with("first");
        let second_id = id.with("second");
        let axis = self.axis;

        let sync_pending = ui
            .try_state::<SplitterState>(id)
            .is_some_and(|response| response.sync_ratio_next_record);
        let synced_ratio = if sync_pending {
            arranged_pane_ratio(ui, first_id, second_id, axis)
        } else {
            None
        };
        let ratio = synced_ratio.unwrap_or_else(|| sanitize_ratio(*self.ratio));
        let mut layout_ratio = ratio;
        let mut resizing = false;
        if !response.disabled {
            // Divider follows the pointer: map the container-local
            // position on the split axis to the first pane's share.
            if divider.left.drag.dragging()
                && let (Some(local), Some(rect)) = (response.pointer_local, response.layout_rect)
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

        // Written only on a change, against the `sync_pending` read above
        // — an absent row reads as `false` there, which is this field's
        // default. A splitter that never resizes mints no row at all.
        let sync_next = resizing || (sync_pending && synced_ratio.is_none());
        if sync_next != sync_pending {
            ui.state_mut::<SplitterState>(id).sync_ratio_next_record = sync_next;
        }

        let bar_fill = if divider.left.drag.dragging() {
            Some(drag_color)
        } else if divider.hovered && !response.disabled {
            Some(hover_color)
        } else {
            None
        };
        // Resize cursor while the divider is hot. Keyed off `dragged`
        // first: mid-drag the pointer routinely leaves the thin bar
        // (`hovered` is also capture-gated), and the cursor must hold
        // until release.
        if bar_fill.is_some() {
            ui.set_cursor(CursorIcon::resize_along(axis));
        }
        let bar_bg = bar_fill.map(Background::fill).unwrap_or_default();
        let rule_bg = Background::fill(rule_color);

        let main_tracks = [
            Track::new(Sizing::share(layout_ratio)),
            Track::fixed(rule_thickness),
            Track::new(Sizing::share(1.0 - layout_ratio)),
        ];
        let cross_tracks = [Track::FILL];
        let [rows, cols] = axis.rows_cols(&main_tracks[..], &cross_tracks[..]);
        let grid_def_id = ui.push_grid_def(rows, cols);
        // The middle track *is* the seam, so the grid owes no spacing of
        // its own — a caller's `gap` would push the panes off the rule.
        widget.node.gaps.set_gap(0.0);
        widget.node.gaps.set_line_gap(0.0);
        widget.node.set_mode(LayoutMode::Grid(grid_def_id));
        widget.record(ui, None, |ui| {
            pane(ui, first_id, axis, 0, |ui| body(ui, SplitHalf::First));

            let mut rule = Node::leaf()
                .id(id.with("rule"))
                .size((Sizing::FILL, Sizing::FILL));
            rule.grid.set_main(axis, 1);
            ui.widget(rule).record(ui, Some(&rule_bg), |_| {});

            pane(ui, second_id, axis, 2, |ui| body(ui, SplitHalf::Second));

            // The grab bar overhangs the seam on the split axis only, so
            // its inset is main-axis with nothing across.
            let inset = (rule_thickness - thickness) * 0.5;
            let mut bar = Node::leaf()
                .id(divider_id)
                .sense(Sense::DRAG)
                .size((Sizing::FILL, Sizing::FILL))
                .margin(axis.compose_spacing(inset, 0.0));
            // No `Configure` twin: cell placement there is `(row, col)`, and
            // which of the two this axis means is the whole point here.
            bar.grid.set_main(axis, 1);
            ui.widget(bar).record(ui, Some(&bar_bg), |_| {});
        });

        Response::eager(id, ui, response)
    }
}

impl_configure!(Splitter<'_>);

/// One pane: a clipped ZStack filling its Grid cell.
fn pane(ui: &mut Ui, id: WidgetId, axis: Axis, main_cell: u16, body: impl FnOnce(&mut Ui)) {
    let mut el = Node::zstack()
        .id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .clip_rect();
    el.grid.set_main(axis, main_cell);
    ui.widget(el).record(ui, None, body)
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
    (!approx::noop_f32(span)).then(|| sanitize_ratio(first_extent / span))
}

/// A caller-supplied ratio, made safe to use as a `Fill` weight. The
/// same screen `Sizing::split` applies, under this widget's own neutral:
/// a splitter with no ratio to honour opens centred, where a progress
/// bar with none reads empty.
fn sanitize_ratio(r: f32) -> f32 {
    r.unit_fraction_or(0.5)
}

/// Map a container-local pointer coordinate on the split axis to the
/// first pane's share of the free space (`extent − reserved`, where
/// `reserved` is the seam the rule occupies in layout). The seam center
/// follows the pointer; `min_pane` floors both panes, collapsing to a
/// centered clamp when the free space can't fit two floors. Degenerate
/// extents pin to `0.5`.
fn pointer_to_ratio(pos: f32, extent: f32, reserved: f32, min_pane: f32) -> f32 {
    let span = extent - reserved;
    if approx::noop_f32(span) {
        return 0.5;
    }
    // `floor <= 0.5` by construction, so the clamp can't invert even
    // when `2 * min_pane > span` — it collapses to the centre instead.
    let floor = (min_pane / span).min(0.5);
    pos.band_fraction(extent, reserved)
        .clamp(floor, 1.0 - floor)
}

#[cfg(test)]
mod tests;
