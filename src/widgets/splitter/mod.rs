//! Two panes divided by a draggable rule: the widget, its per-pane bodies, and the
//! split ratio it keeps between frames.

pub(crate) mod split_half;

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::track::Track;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::splitter::SplitterTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::{AxisEnd, AxisKeys, KeyPair};
use crate::widgets::splitter::split_half::SplitHalf;
use crate::window::cursor_icon::CursorIcon;

/// Two panes split by a draggable divider: [`Splitter::row`] side by side,
/// [`Splitter::col`] stacked, [`Splitter::along`] either. The caller owns the split as
/// `ratio`, the first pane's share of the free space, `0..1`. While dragging, the
/// pointer target feeds layout at once and the content-constrained share is written
/// back on the next record. Double-clicking the divider recenters to `0.5`. Visuals
/// come from [`crate::SplitterTheme`] (slot `splitter`).
///
/// One Grid owns the pane tracks and the visible `rule_thickness` seam; the wide
/// grab target is a late-recorded overlay in the rule's cell, so layout places it
/// at the content-constrained boundary without a second pass. [`Splitter::show`]
/// records both panes through one `FnMut` body called with [`SplitHalf::First`]
/// then [`SplitHalf::Second`], so a recursive pane tree can capture its response
/// mutably once.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Splitter<'a> {
    widget: Widget,
    ratio: &'a mut f32,
    axis: Axis,
    min_pane: f32,
    min_ratio: f32,
    style: Option<&'a SplitterTheme>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SplitterState {
    sync_ratio_next_record: bool,
    /// A double-click reset the ratio; the binding takes its arranged result on the
    /// next record, which commits it.
    commit_on_sync: bool,
    /// The seam's center along the axis, in the splitter's px, when the live drag latched: the
    /// seam follows the drag's travel from it, so a press off the seam's center keeps its grab
    /// offset, as a scroll thumb's drag does.
    drag_from: Option<f32>,
}

impl<'a> Splitter<'a> {
    /// Side-by-side panes with a vertical divider bar; `ratio` is the left pane's
    /// share.
    #[track_caller]
    pub fn row(ratio: &'a mut f32) -> Self {
        Self::along(Axis::X, ratio)
    }

    /// Stacked panes with a horizontal divider bar; `ratio` is the top pane's
    /// share.
    #[track_caller]
    pub fn col(ratio: &'a mut f32) -> Self {
        Self::along(Axis::Y, ratio)
    }

    /// Panes laid along `axis`, with the divider bar across it; `ratio` is the first pane's
    /// share. [`Axis::X`] is [`Self::row`].
    #[track_caller]
    pub fn along(axis: Axis, ratio: &'a mut f32) -> Self {
        Self {
            widget: Widget::grid(),
            ratio,
            axis,
            min_pane: 0.0,
            min_ratio: 0.0,
            style: None,
        }
    }

    /// Floor either pane's split-axis extent at `px`, a *length*, while dragging.
    /// Default `0.0`.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::is_length).
    #[track_caller]
    pub const fn min_pane(mut self, px: f32) -> Self {
        self.min_pane = domain::length(px);
        self
    }

    /// Floor either pane's share of the free space at `ratio` while dragging, beside
    /// [`Self::min_pane`]'s length, for an owner that clamps the ratio it stores: a drag
    /// past that clamp would snap back on release. Default `0.0`.
    ///
    /// # Panics
    ///
    /// Panics unless `ratio` is in `0.0..=0.5`, as two floors above a half cannot both hold.
    #[track_caller]
    pub const fn min_ratio(mut self, ratio: f32) -> Self {
        assert!(ratio >= 0.0, "a ratio floor must be at least 0.0");
        assert!(ratio <= 0.5, "a ratio floor must be at most 0.5");
        self.min_ratio = ratio;
        self
    }

    /// Per-instance override of [`Theme::splitter`](crate::Theme::splitter).
    pub fn style(mut self, s: impl Into<Option<&'a SplitterTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record both panes and the divider between them; `body` runs once per
    /// [`SplitHalf`]. The divider drags the bound `ratio`; as for
    /// [`Slider`](crate::Slider), `changed` is set on every frame the ratio moves
    /// and `committed` on the frame a drag releases or a double-click reset lands.
    ///
    /// **The binding trails the drawn divider by one frame while it moves**: layout
    /// follows the pointer at once, but the ratio written back is the share the
    /// panes were arranged at, which only layout knows, so a content floor is never
    /// overridden.
    pub fn show(mut self, ui: &mut Ui, mut body: impl FnMut(&mut Ui, SplitHalf)) -> ValueResponse {
        self.widget
            .configure()
            .default_size(SizeSpec::FILL)
            .default_clip(ClipMode::Rect);
        let response = self.widget.response(ui);
        let id = response.id;
        let input = *self.ratio;

        let theme = self.style.unwrap_or(&ui.theme().splitter);
        let grab_thickness = domain::length_at_least(theme.grab_thickness, 1.0);
        let rule_thickness = domain::length_at_least(theme.rule_thickness, 0.0);
        let rule_color = theme.rule;
        let hovered_color = theme.rule_hovered;
        let active_color = theme.rule_active;

        let divider_id = id.with("divider");
        // The splitter's own `disabled` reaches its divider through the cascade only next
        // frame; folded here, a splitter disabled mid-drag stops painting and steering at once.
        let mut divider = ui.response_for(divider_id);
        divider.merge_disabled(response.disabled);
        // A Tab stop taking the `CARET` keys as WAI-ARIA's window splitter does;
        // built here so it reads its keys before the grid records.
        let mut divider_widget = Widget::leaf()
            .id(divider_id)
            .sense(Sense::DRAG)
            .focusable(true)
            .input_scope(KeyFilter::CARET);
        let axis = self.axis;
        let panes = Panes {
            first: id.with("first"),
            second: id.with("second"),
            axis,
        };

        let state = ui.state::<SplitterState>(id).copied().unwrap_or_default();
        let sync_pending = state.sync_ratio_next_record;
        let synced_ratio = if sync_pending {
            panes.arranged_ratio(ui)
        } else {
            None
        };
        let ratio = synced_ratio.unwrap_or_else(|| sanitize_ratio(*self.ratio));
        let mut layout_ratio = ratio;
        let mut resizing = false;
        let mut reset = false;
        let mut drag_from = None;
        // The seam's center along the axis for a splitter `extent` px long.
        let center = |extent: f32| ratio * (extent - rule_thickness) + rule_thickness * 0.5;
        if !response.disabled {
            if let Some(travel) = divider.left.drag.delta()
                && let Some(rect) = response.layout_rect
            {
                let extent = axis.main(rect.size);
                let seam = Seam {
                    extent,
                    reserved: rule_thickness,
                    min_pane: self.min_pane,
                    min_ratio: self.min_ratio,
                };
                let from = state.drag_from.unwrap_or_else(|| center(extent));
                drag_from = Some(from);
                layout_ratio = seam.ratio_at(from + axis.main_v(travel));
                resizing = true;
            }
            if divider.double_clicked() {
                layout_ratio = 0.5;
                resizing = true;
                reset = true;
            }
            // WAI-ARIA window splitter keys: arrows move the seam a step (ten with Shift), Home/End
            // go to either end, through the drag's mapping (same floors), committed
            // on the next sync like a reset.
            if ui.is_focus_within(divider_id)
                && let Some(rect) = response.layout_rect
            {
                let extent = axis.main(rect.size);
                let at = center(extent);
                let keys = match axis {
                    Axis::X => KEYS_X,
                    Axis::Y => KEYS_Y,
                };
                let travel = keys.read(&mut divider_widget, ui);
                let target = match travel.end {
                    Some(AxisEnd::Start) => Some(0.0),
                    Some(AxisEnd::End) => Some(extent),
                    None => (travel.steps != 0).then_some(at + travel.steps as f32 * KEY_STEP),
                };
                if let Some(pos) = target {
                    let seam = Seam {
                        extent,
                        reserved: rule_thickness,
                        min_pane: self.min_pane,
                        min_ratio: self.min_ratio,
                    };
                    layout_ratio = seam.ratio_at(pos);
                    resizing = true;
                    reset = true;
                }
            }
        }
        *self.ratio = ratio;
        let changed = !domain::is_approx_zero(ratio - input);
        let synced = synced_ratio.is_some();
        let committed =
            !response.disabled && (divider.left.drag.stopped() || (state.commit_on_sync && synced));

        // Written only on a change against `state` (an absent row reads as the
        // default), so a splitter that never resizes mints no row.
        let next = SplitterState {
            sync_ratio_next_record: resizing || (sync_pending && !synced),
            commit_on_sync: reset || (state.commit_on_sync && !synced),
            drag_from,
        };
        if next != state {
            ui.with_state::<SplitterState, _>(id, |_, s| *s = next);
        }

        let bar_fill = if divider.left.drag.is_live() {
            Some(active_color)
        } else if divider.hovered() {
            Some(hovered_color)
        } else {
            None
        };
        // Resize cursor while the divider is hot, keyed off the drag first:
        // mid-drag the pointer leaves the thin bar and `hovered` is capture-gated.
        if bar_fill.is_some() {
            ui.request_cursor(CursorIcon::resize_along(axis));
        }
        let bar_chrome = bar_fill.map_or_default(Chrome::new);
        let rule_chrome = Chrome::new(rule_color);

        let main_tracks = [
            Track::fill(layout_ratio),
            Track::fixed(rule_thickness),
            Track::fill(1.0 - layout_ratio),
        ];
        let cross_tracks = [Track::FILL];
        let [rows, cols] = axis.rows_cols(&main_tracks[..], &cross_tracks[..]);
        self.widget.grid_tracks(ui, rows, cols);
        // The middle track *is* the seam, so the grid owes no spacing; a caller's
        // `gap` would push the panes off the rule.
        self.widget.configure().gap(0.0).line_gap(0.0);
        self.widget.record(ui, None, |ui| {
            panes.record(ui, SplitHalf::First, |ui| body(ui, SplitHalf::First));

            Widget::leaf()
                .id(id.with("rule"))
                .size(SizeSpec::FILL)
                .grid_cell(GridCell::along(axis, 1))
                .record(ui, Some(&rule_chrome), |_| {});

            panes.record(ui, SplitHalf::Second, |ui| body(ui, SplitHalf::Second));

            let inset = (rule_thickness - grab_thickness) * 0.5;
            divider_widget
                .size(SizeSpec::FILL)
                .margin(axis.compose_spacing(inset, 0.0))
                .grid_cell(GridCell::along(axis, 1))
                .record(ui, Some(&bar_chrome), |_| {});
        });

        ValueResponse {
            response,
            changed,
            committed,
        }
    }
}

impl Configure for Splitter<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// The two panes and the axis they split along.
#[derive(Clone, Copy, Debug)]
struct Panes {
    first: WidgetId,
    second: WidgetId,
    axis: Axis,
}

impl Panes {
    /// Record the `half` pane in its grid cell, either side of the rule's.
    fn record(self, ui: &mut Ui, half: SplitHalf, body: impl FnOnce(&mut Ui)) {
        let (id, main_cell) = match half {
            SplitHalf::First => (self.first, 0),
            SplitHalf::Second => (self.second, 2),
        };
        Widget::zstack()
            .id(id)
            .size(SizeSpec::FILL)
            .clip_rect()
            .grid_cell(GridCell::along(self.axis, main_cell))
            .record(ui, None, body);
    }

    /// Recover the first pane's effective share after layout applied both panes'
    /// content floors.
    fn arranged_ratio(self, ui: &Ui) -> Option<f32> {
        let first = ui.response_for(self.first).layout_rect?;
        let second = ui.response_for(self.second).layout_rect?;
        let first_extent = self.axis.main(first.size);
        let second_extent = self.axis.main(second.size);
        let span = first_extent + second_extent;
        (!domain::is_invisible(span)).then(|| sanitize_ratio(first_extent / span))
    }
}

/// A caller-supplied ratio made safe as a `Fill` weight; unlike a progress bar, an
/// invalid one opens centered.
const fn sanitize_ratio(r: f32) -> f32 {
    domain::fraction_or(r, 0.5)
}

const KEY_STEP: f32 = 10.0;

/// WAI-ARIA window splitter keys on a row: Left and Right move the seam, Home and End go to
/// either end.
const KEYS_X: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    }],
    page: None,
    ends: Some(KEY_ENDS),
};

/// [`KEYS_X`] on a column, with Up and Down.
const KEYS_Y: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowUp,
        forward: Key::ArrowDown,
    }],
    page: None,
    ends: Some(KEY_ENDS),
};

const KEY_ENDS: KeyPair = KeyPair {
    back: Key::Home,
    forward: Key::End,
};

/// The split axis as the divider maps it: the container's `extent`, the rule's
/// `reserved` share of it, and the floors both panes keep while dragging, as a length
/// and as a share.
#[derive(Clone, Copy, Debug)]
struct Seam {
    extent: f32,
    reserved: f32,
    min_pane: f32,
    min_ratio: f32,
}

impl Seam {
    /// Map a container-local coordinate `pos` to the first pane's share of the free
    /// space (`extent − reserved`); the seam center follows `pos` and the larger of
    /// `min_pane` and `min_ratio` floors both panes, collapsing to a centered clamp when
    /// two floors do not fit. Degenerate extents pin to `0.5`.
    fn ratio_at(self, pos: f32) -> f32 {
        let span = self.extent - self.reserved;
        if domain::is_invisible(span) {
            return 0.5;
        }
        let floor = (self.min_pane / span).max(self.min_ratio).min(0.5);
        domain::band_fraction(pos, self.extent, self.reserved).clamp(floor, 1.0 - floor)
    }
}

#[cfg(test)]
mod tests;
