//! Differential oracles: each retained result (measure cache, incremental cascade, damage diff) checked against one computed from scratch, since a gate that misses an input paints stale pixels silently. [`Oracle::check_frame`] runs them after a frame.

use crate::cascade::Cascade;
use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::engine::CascadeEngine;
use crate::cascade::internals::OwnedPaint;
use crate::damage::Damage;
use crate::internals::harness::UiHarness;
use crate::layout::Layout;
use crate::layout::engine::LayoutEngine;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::render_plan::RenderPlan;
use crate::ui::frame_report::FrameReport;
use std::cmp;
use std::mem;

/// The previous frame's paint rows, and the scratch the next check fills.
#[derive(Debug, Default)]
pub(crate) struct Oracle {
    prev: Vec<OwnedPaint>,
    curr: Vec<OwnedPaint>,
    /// Whether `prev` holds a frame yet; the first checked frame has nothing to diff.
    primed: bool,
}

impl Oracle {
    /// Run every oracle against the frame `h` just ran, which produced `report`.
    ///
    /// # Panics
    ///
    /// Panics, naming the node or row, when a retained result differs
    /// from its cold counterpart or the damage misses a changed row.
    pub(crate) fn check_frame(&mut self, h: &UiHarness, report: &FrameReport) {
        assert_layout_matches_cold(h);
        assert_cascade_matches_cold(h);
        self.check_damage(h, report);
    }

    /// Rows in one frame and not the other, and overlaps whose paint order flipped, must sit inside the painted damage; compared per owner as a multiset of `(hash, screen)`.
    fn check_damage(&mut self, h: &UiHarness, report: &FrameReport) {
        self.curr.clear();
        h.ui.cascade().owned_paints(h.ui.forest(), &mut self.curr);
        if self.primed {
            let surface = h.ui.display().logical_rect();
            let margin = RenderPlan::cull_margin(h.ui.display().scale_factor());
            let damage: Vec<Rect> = match report.plan.as_ref().map(|plan| &plan.damage) {
                None => Vec::new(),
                Some(Damage::Full) => vec![surface],
                Some(Damage::Partial(collapsed)) => collapsed
                    .region
                    .iter_rects()
                    .map(|rect| rect.inflated(margin))
                    .collect(),
            };
            sort_rows(&mut self.prev);
            sort_rows(&mut self.curr);
            let RowDiff { changed, mut kept } = diff_rows(&self.prev, &self.curr);
            for (row, side) in changed {
                let Some(visible) = row.screen.intersect(surface) else {
                    continue;
                };
                assert!(
                    covered(visible, &damage),
                    "{side} paint row of {:?} at {:?} (hash {:?}) changed but lies outside \
                     this frame's damage {damage:?}",
                    row.owner,
                    row.screen,
                    row.hash,
                );
            }
            kept.sort_unstable_by_key(|pair| pair.curr_rank);
            for (later, b) in kept.iter().enumerate() {
                for a in &kept[..later] {
                    if a.prev_rank < b.prev_rank {
                        continue;
                    }
                    let Some(overlap) = a.screen.intersect(b.screen) else {
                        continue;
                    };
                    let Some(visible) = overlap.intersect(surface) else {
                        continue;
                    };
                    assert!(
                        covered(visible, &damage),
                        "rows at {:?} and {:?} swapped paint order but their overlap \
                         {visible:?} lies outside this frame's damage {damage:?}",
                        a.screen,
                        b.screen,
                    );
                }
            }
        }
        mem::swap(&mut self.prev, &mut self.curr);
        self.primed = true;
    }
}

/// Lay the forest out again with a cold engine and assert every node's result equals the frame's.
fn assert_layout_matches_cold(h: &UiHarness) {
    let mut engine = LayoutEngine::new(h.ui.shaper().clone());
    let mut cold = Layout::default();
    let store = &h.ui.forest().record_store;
    let interned_text = store.interned_text();
    engine.run(
        h.ui.forest(),
        &interned_text,
        h.ui.display().logical_rect(),
        &mut cold,
    );
    for (layer, tree) in h.ui.forest().trees.iter_paint_order() {
        let (warm, cold) = (h.ui.layout(layer), &cold[layer]);
        for (node, id) in tree.records.widget_id().iter().enumerate() {
            let at = || format!("{layer:?} node {node} ({id:?})");
            assert_eq!(warm.rect[node], cold.rect[node], "rect of {}", at());
            assert_eq!(
                warm.scroll_content[node],
                cold.scroll_content[node],
                "scroll content of {}",
                at(),
            );
            let runs = |layout: &LayerLayout| {
                layout.text_shapes[layout.text_spans[node].range()]
                    .iter()
                    .map(|shaped: &ShapedText| (shaped.extent.size, shaped.key))
                    .collect::<Vec<_>>()
            };
            assert_eq!(runs(warm), runs(cold), "shaped text of {}", at());
        }
    }
}

/// Rebuild the cascade in full over the frame's layout and assert the retained one equals it; the layout oracle reports layout disagreements.
fn assert_cascade_matches_cold(h: &UiHarness) {
    let key = CascadeKey::new(
        h.ui.forest(),
        h.ui.layout_tables(),
        h.ui.display(),
        h.ui.font_epoch(),
    );
    let mut cold = Cascade::default();
    CascadeEngine::default().run(
        h.ui.forest(),
        h.ui.layout_tables(),
        h.ui.display(),
        &key,
        &mut cold,
    );
    assert_eq!(
        h.ui.cascade().key,
        cold.key,
        "the key the cascade was built from"
    );
    h.ui.cascade().assert_same_as(&cold, h.ui.forest());
    assert_eq!(
        h.ui.cascade().by_id,
        h.ui.forest().ids.last_frame(),
        "id lookup"
    );
}

fn sort_rows(rows: &mut [OwnedPaint]) {
    rows.sort_unstable_by_key(row_key);
}

/// The order two frames' rows merge in: owner, content, exact screen rect.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    owner: u64,
    hash: u64,
    screen: [u32; 4],
}

fn row_key(row: &OwnedPaint) -> RowKey {
    RowKey {
        owner: row.owner.0,
        hash: row.hash.0,
        screen: [
            row.screen.min.x.to_bits(),
            row.screen.min.y.to_bits(),
            row.screen.size.w.to_bits(),
            row.screen.size.h.to_bits(),
        ],
    }
}

/// A row in both frames, with its place in each frame's paint order.
#[derive(Debug)]
struct KeptRow {
    screen: Rect,
    prev_rank: u32,
    curr_rank: u32,
}

#[derive(Debug)]
struct RowDiff<'a> {
    changed: Vec<(&'a OwnedPaint, &'static str)>,
    kept: Vec<KeptRow>,
}

/// Merge two sorted multisets of rows.
fn diff_rows<'a>(prev: &'a [OwnedPaint], curr: &'a [OwnedPaint]) -> RowDiff<'a> {
    let mut diff = RowDiff {
        changed: Vec::new(),
        kept: Vec::new(),
    };
    let (mut i, mut j) = (0, 0);
    while i < prev.len() || j < curr.len() {
        match (prev.get(i), curr.get(j)) {
            (Some(a), Some(b)) => match row_key(a).cmp(&row_key(b)) {
                cmp::Ordering::Equal => {
                    diff.kept.push(KeptRow {
                        screen: b.screen,
                        prev_rank: a.rank,
                        curr_rank: b.rank,
                    });
                    i += 1;
                    j += 1;
                }
                cmp::Ordering::Less => {
                    diff.changed.push((a, "previous"));
                    i += 1;
                }
                cmp::Ordering::Greater => {
                    diff.changed.push((b, "current"));
                    j += 1;
                }
            },
            (Some(a), None) => {
                diff.changed.push((a, "previous"));
                i += 1;
            }
            (None, Some(b)) => {
                diff.changed.push((b, "current"));
                j += 1;
            }
            (None, None) => unreachable!("the loop condition admits one side"),
        }
    }
    diff
}

/// Whether `rect` lies inside the union of `cover`, exactly.
fn covered(rect: Rect, cover: &[Rect]) -> bool {
    if rect.size.w <= 0.0 || rect.size.h <= 0.0 {
        return true;
    }
    let Some((first, rest)) = cover.split_first() else {
        return false;
    };
    let Some(overlap) = rect.intersect(*first) else {
        return covered(rect, rest);
    };
    let (x0, y0, x1, y1) = (rect.min.x, rect.min.y, rect.max().x, rect.max().y);
    let (ox0, oy0, ox1, oy1) = (
        overlap.min.x,
        overlap.min.y,
        overlap.max().x,
        overlap.max().y,
    );
    let pieces = [
        Rect::new(x0, y0, x1 - x0, oy0 - y0),
        Rect::new(x0, oy1, x1 - x0, y1 - oy1),
        Rect::new(x0, oy0, ox0 - x0, oy1 - oy0),
        Rect::new(ox1, oy0, x1 - ox1, oy1 - oy0),
    ];
    pieces.into_iter().all(|piece| covered(piece, rest))
}

#[cfg(test)]
mod tests {
    use super::covered;
    use crate::primitives::geometry::rect::Rect;

    #[test]
    fn coverage_is_exact_over_a_union() {
        let left = Rect::new(0.0, 0.0, 50.0, 100.0);
        let right = Rect::new(50.0, 0.0, 50.0, 100.0);
        let whole = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(covered(whole, &[left, right]), "two halves cover the whole");
        assert!(!covered(whole, &[left]), "one half does not");
        assert!(!covered(whole, &[]), "nothing covers nothing but empty");
        assert!(
            covered(Rect::new(10.0, 10.0, 0.0, 5.0), &[]),
            "an empty rect is covered"
        );
        let gap = Rect::new(52.0, 0.0, 48.0, 100.0);
        assert!(!covered(whole, &[left, gap]), "a 2 px seam is uncovered");
    }
}
