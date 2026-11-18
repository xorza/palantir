//! Two panes divided by a draggable rule: the widget, its per-pane bodies, and the
//! split size it keeps between frames.

pub(crate) mod split_half;
pub(crate) mod split_size;

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
use crate::widgets::splitter::split_size::SplitSize;
use crate::window::cursor_icon::CursorIcon;

/// Two panes split by a draggable divider: [`Splitter::row`] side by side,
/// [`Splitter::col`] stacked, [`Splitter::along`] either. The caller owns the split as a
/// [`SplitSize`]: a ratio, the first pane's share of the free space, or one pane's pixels. A
/// drag keeps its shape. While dragging, the pointer target feeds layout at once and the
/// content-constrained size is written back on the next record. Double-clicking the divider
/// recenters it, a ratio to `0.5` and a fixed pane to half the split. Visuals come from
/// [`crate::SplitterTheme`] (slot `splitter`).
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
    size: &'a mut SplitSize,
    axis: Axis,
    min_pane: f32,
    min_ratio: f32,
    style: Option<&'a SplitterTheme>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SplitterState {
    sync_size_next_record: bool,
    /// A double-click reset the size; the binding takes its arranged result on the
    /// next record, which commits it.
    commit_on_sync: bool,
    /// The seam's center along the axis, in the splitter's px, when the live drag latched: the
    /// seam follows the drag's travel from it, so a press off the seam's center keeps its grab
    /// offset, as a scroll thumb's drag does.
    drag_from: Option<f32>,
}

impl<'a> Splitter<'a> {
    /// Side-by-side panes with a vertical divider bar, split as `size`.
    #[track_caller]
    pub fn row(size: &'a mut SplitSize) -> Self {
        Self::along(Axis::X, size)
    }

    /// Stacked panes with a horizontal divider bar, split as `size`.
    #[track_caller]
    pub fn col(size: &'a mut SplitSize) -> Self {
        Self::along(Axis::Y, size)
    }

    /// Panes laid along `axis`, with the divider bar across it, split as `size`. [`Axis::X`] is
    /// [`Self::row`].
    #[track_caller]
    pub fn along(axis: Axis, size: &'a mut SplitSize) -> Self {
        Self {
            widget: Widget::grid(),
            size,
            axis,
            min_pane: 0.0,
            min_ratio: 0.0,
            style: None,
        }
    }

    /// Floor either pane's split-axis extent at `px`, a *length*, while dragging, and under a
    /// fixed [`SplitSize`] at every size: the other pane keeps it, and the fixed pane gives way
    /// toward it. Default `0.0`.
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
    /// [`SplitHalf`]. The divider drags the bound size; as for
    /// [`Slider`](crate::Slider), `changed` is set on every frame the size moves
    /// and `committed` on the frame a drag releases or a double-click reset lands.
    ///
    /// **The binding trails the drawn divider by one frame while it moves**: layout
    /// follows the pointer at once, but the size written back is the one the
    /// panes were arranged at, which only layout knows, so a content floor is never
    /// overridden.
    pub fn show(mut self, ui: &mut Ui, mut body: impl FnMut(&mut Ui, SplitHalf)) -> ValueResponse {
        self.widget
            .configure()
            .default_size(SizeSpec::FILL)
            .default_clip(ClipMode::Rect);
        let response = self.widget.response(ui);
        let id = response.id;
        let input = *self.size;

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
        let sync_pending = state.sync_size_next_record;
        let synced_size = if sync_pending {
            panes.arranged_size(ui, input)
        } else {
            None
        };
        let size = synced_size.unwrap_or_else(|| input.sanitized());
        let mut layout_size = size;
        let mut resizing = false;
        let mut reset = false;
        let mut drag_from = None;
        let min_pane = self.min_pane;
        // The seam's center along the axis for a splitter `extent` px long, where `size` draws it.
        let center =
            |extent: f32| size.first_of(extent - rule_thickness, min_pane) + rule_thickness * 0.5;
        // The axis as last frame arranged it, which both the drag and the keys map through.
        let seam = response.layout_rect.map(|rect| Seam {
            extent: axis.main(rect.size),
            reserved: rule_thickness,
            min_pane: self.min_pane,
            min_ratio: self.min_ratio,
        });
        if !response.disabled {
            if let Some(travel) = divider.left.drag.delta()
                && let Some(seam) = seam
            {
                let from = state.drag_from.unwrap_or_else(|| center(seam.extent));
                drag_from = Some(from);
                layout_size = seam.size_at(size, from + axis.main_v(travel));
                resizing = true;
            }
            if divider.double_clicked() {
                layout_size = match (size, seam) {
                    (SplitSize::Fixed { half, .. }, Some(seam)) => SplitSize::Fixed {
                        half,
                        px: seam.span() * 0.5,
                    },
                    (SplitSize::Fixed { .. }, None) => size,
                    (SplitSize::Ratio(_), _) => SplitSize::HALF,
                };
                resizing = true;
                reset = true;
            }
            // WAI-ARIA window splitter keys: arrows move the seam a step (ten with Shift), Home/End
            // go to either end, through the drag's mapping (same floors), committed
            // on the next sync like a reset.
            if ui.is_focus_within(divider_id)
                && let Some(seam) = seam
            {
                let extent = seam.extent;
                let at = center(extent);
                let keys = match axis {
                    Axis::X => AxisKeys::SCROLL_X_ALONE,
                    Axis::Y => KEYS_Y,
                };
                let travel = keys.read(&mut divider_widget, ui);
                let target = match travel.end {
                    Some(AxisEnd::Start) => Some(0.0),
                    Some(AxisEnd::End) => Some(extent),
                    None => (travel.steps != 0).then_some(at + travel.steps as f32 * KEY_STEP),
                };
                if let Some(pos) = target {
                    layout_size = seam.size_at(size, pos);
                    resizing = true;
                    reset = true;
                }
            }
        }
        *self.size = size;
        let changed = size.differs(input);
        let synced = synced_size.is_some();
        let committed =
            !response.disabled && (divider.left.drag.stopped() || (state.commit_on_sync && synced));

        // Written only on a change against `state` (an absent row reads as the
        // default), so a splitter that never resizes mints no row.
        let next = SplitterState {
            sync_size_next_record: resizing || (sync_pending && !synced),
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

        let [first_track, second_track] = layout_size.tracks(min_pane);
        let main_tracks = [first_track, Track::fixed(rule_thickness), second_track];
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

    /// The size, of `shape`'s shape, the panes were arranged at once layout applied both
    /// panes' content floors.
    fn arranged_size(self, ui: &Ui, shape: SplitSize) -> Option<SplitSize> {
        let first = ui.response_for(self.first).layout_rect?;
        let second = ui.response_for(self.second).layout_rect?;
        let first_extent = self.axis.main(first.size);
        let span = first_extent + self.axis.main(second.size);
        (!domain::is_invisible(span)).then(|| shape.with_first(first_extent, span))
    }
}

const KEY_STEP: f32 = 10.0;

/// WAI-ARIA window splitter keys on a column: Up and Down move the seam, Home and End go to
/// either end, as [`AxisKeys::SCROLL_X_ALONE`] does on a row.
const KEYS_Y: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowUp,
        forward: Key::ArrowDown,
    }],
    page: None,
    ends: Some(KeyPair {
        back: Key::Home,
        forward: Key::End,
    }),
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
    /// The space past the rule.
    fn span(self) -> f32 {
        self.extent - self.reserved
    }

    /// `shape`'s shape with the seam at the container-local `pos`, through [`Self::ratio_at`]'s
    /// floors.
    fn size_at(self, shape: SplitSize, pos: f32) -> SplitSize {
        let span = self.span();
        shape.with_first(self.ratio_at(pos) * span, span)
    }

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
