use super::cmd_buffer::{EnterPatch, RenderCmdBuffer};
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, clip_mode::ClipMode};
use crate::layout::{cache::AvailableKey, result::LayoutResult};
use crate::primitives::{
    corners::Corners, rect::Rect, size::Size, spacing::Spacing, transform::TranslateScale,
};
use crate::shape::Shape;
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree, TreeOp, node_hash::NodeHash};
use crate::ui::cascade::CascadeResult;
use cache::EncodeCache;

/// Bookkeeping captured before recursing so we can write the cached
/// subtree back after children have appended their cmds. `cmd_lo` /
/// `data_lo` snapshot `out`'s arena lengths at entry; the hi ends are
/// read after recursion to form the subtree's spans. Mirrors
/// `composer::SubtreeFrame` (same shape, different per-cache key
/// fields).
struct SubtreeFrame {
    wid: WidgetId,
    subtree_hash: NodeHash,
    avail: AvailableKey,
    cmd_lo: u32,
    data_lo: u32,
    enter_patch: EnterPatch,
}

/// Skip cache lookup + write for subtrees of `<=` this many nodes.
/// A small subtree's encode work (a handful of `draw_rect` /
/// `draw_text`) is cheaper than the hashmap miss + insert + marker
/// emission it would replace. The win shows on cold / forced-miss
/// frames where every parent miss falls through to per-leaf cache I/O.
///
/// Also gates `EnterSubtree`/`ExitSubtree` marker emission — they're
/// only useful for subtrees the composer cache might want to skip,
/// and small subtrees never will.
const TINY_SUBTREE_THRESHOLD: u32 = 4;

/// Walk the tree pre-order and emit logical-px paint commands. No GPU work,
/// no scale/snap math — that lives in the backend's process step. Pure
/// function over `(&Tree, &LayoutResult, &Cascades)`, so the same call works
/// in unit tests with no device. Reads invisibility cascade from `Cascades`
/// so encoder and hit-index can't drift.
///
/// `damage_filter` enables Step 5 of the damage-rendering plan: when
/// `Some(rect)`, leaf paint commands (`DrawRect`/`DrawText`) are skipped
/// for nodes whose arranged rect doesn't intersect the filter. Clip and
/// transform push/pop pairs are *always* emitted so descendant scissor
/// state and group boundaries (composer text↔quad split) stay correct.
/// `None` paints everything — used for the first frame, full-repaint
/// fallback, and existing tests.
///
/// Owns the cross-frame subtree-skip cache (`EncodeCache`) and exposes
/// the encode entry point. The output [`RenderCmdBuffer`] stays on
/// [`Frontend`](crate::renderer::frontend::Frontend) since the composer also reads
/// it; the cache lives here because nothing else in the frontend touches
/// it.
///
/// Cache **reads** run on every frame (full and damage-filtered): a
/// cached replay reproduces the prior full-paint cmd stream byte-for-byte
/// and is correctly scissored by the backend. Cache **writes** are
/// gated on `damage_filter.is_none()` — a partial-paint frame skips
/// per-leaf shapes outside the dirty region, so recording its output
/// would lie about the snapshot covering the full subtree. Snapshot age
/// is therefore bounded by the last full-paint frame, not the last
/// frame. See `cache::EncodeCache` for the cascade-not-in-key argument.
#[derive(Default)]
pub(crate) struct Encoder {
    pub(crate) cache: EncodeCache,
    pub(crate) cmds: RenderCmdBuffer,
}

impl Encoder {
    /// Encode `tree` into the encoder's owned command buffer using last
    /// frame's cache for subtree skips (when `damage_filter.is_none()`),
    /// and return a borrow of the freshly-encoded result.
    pub(crate) fn encode(
        &mut self,
        tree: &Tree,
        layout: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<Rect>,
    ) -> &RenderCmdBuffer {
        self.cmds.clear();
        if let Some(root) = tree.root() {
            let mut shape_cursor = 0usize;
            encode_node(
                tree,
                layout,
                cascades,
                damage_filter,
                &mut self.cache,
                root,
                &mut shape_cursor,
                &mut self.cmds,
            );
        }
        &self.cmds
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.cache.sweep_removed(removed);
    }
}

/// Emit one shape at `owner_rect`, advancing the global shape cursor.
/// Pulled out of `encode_node` so the kinds-stream walk can call it
/// from the depth-0 `Shape` branch without duplicating the per-variant
/// match.
fn emit_one_shape(
    tree: &Tree,
    layout: &LayoutResult,
    id: NodeId,
    owner_rect: Rect,
    shape: &Shape,
    out: &mut RenderCmdBuffer,
) {
    match shape {
        Shape::RoundedRect {
            radius,
            fill,
            stroke,
        } => {
            out.draw_rect(owner_rect, *radius, *fill, *stroke);
        }
        Shape::SubRect {
            local_rect,
            radius,
            fill,
            stroke,
        } => {
            let r = Rect {
                min: owner_rect.min + local_rect.min,
                size: local_rect.size,
            };
            out.draw_rect(r, *radius, *fill, *stroke);
        }
        Shape::Text { color, align, .. } => {
            let Some(shaped) = layout.text_shapes[id.index()] else {
                return;
            };
            if shaped.key.is_invalid() {
                tracing::trace!(?shape, "encoder: dropping text with invalid key");
                return;
            }
            let inner = owner_rect.deflated_by(tree.records.layout()[id.index()].padding);
            out.draw_text(
                align_text_in(inner, shaped.measured, *align),
                *color,
                shaped.key,
            );
        }
        Shape::Line { .. } => {
            tracing::trace!(?shape, "encoder: dropping unsupported shape");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_node(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &CascadeResult,
    damage_filter: Option<Rect>,
    cache: &mut EncodeCache,
    id: NodeId,
    shape_cursor: &mut usize,
    out: &mut RenderCmdBuffer,
) {
    // Hidden / Collapsed: paint nothing for this node or its subtree.
    // The cascade table already composed self + ancestors; recursing skips
    // the whole subtree because we early-return at the top of every node.
    if cascades.rows[id.index()].invisible {
        // Still advance the global shape cursor past this subtree's
        // shapes so siblings see the right offset.
        *shape_cursor += tree.records.shapes()[id.index()].len as usize;
        return;
    }

    // Cross-frame subtree-skip cache (Phase 3). Reads run on every
    // frame (full or damage-filtered): replaying a cached subtree
    // restores the *complete* cmd stream from the prior full-paint
    // frame, which is byte-identical to a fresh full encode and is
    // correctly scissored downstream by the backend's damage rect.
    // Writes are gated on full-paint frames only — a damage-filtered
    // frame skips per-leaf paint for nodes outside the dirty region,
    // so writing back would record a partial subtree and lie about
    // coverage. Cache snapshot age is therefore bounded by the *last
    // full-paint* frame, not the last frame. See `cache::EncodeCache`.
    let subtree_size = tree.records.end()[id.index()] - id.index() as u32;
    let cache_key = if subtree_size > TINY_SUBTREE_THRESHOLD {
        layout.available_q(id).map(|avail| {
            (
                tree.records.widget_id()[id.index()],
                tree.subtree_hash(id),
                avail,
            )
        })
    } else {
        None
    };

    if let Some((wid, hash, avail)) = cache_key
        && cache.try_replay(wid, hash, avail, out, layout.rect[id.index()].min)
    {
        *shape_cursor += tree.records.shapes()[id.index()].len as usize;
        return;
    }

    // Bracket cache-eligible subtrees with `EnterSubtree`/`ExitSubtree`
    // markers on full-paint frames only. The markers go *inside* the
    // snapshot range (cmd_lo is captured before `push_enter_subtree`
    // so the open cmd is at index `cmd_lo`; the close is the last cmd
    // in the range). Composer reads `EnterSubtree` to attempt a splice
    // (fast-forwarding past the matching `ExitSubtree` on a hit) and
    // uses `ExitSubtree` to write the snapshot back on a miss.
    let cache_pending = if damage_filter.is_none()
        && let Some((wid, subtree_hash, avail)) = cache_key
    {
        let cmd_lo = out.kinds.len() as u32;
        let data_lo = out.data.len() as u32;
        let enter_patch = out.push_enter_subtree(wid, subtree_hash, avail);
        Some(SubtreeFrame {
            wid,
            subtree_hash,
            avail,
            cmd_lo,
            data_lo,
            enter_patch,
        })
    } else {
        None
    };

    let rect = layout.rect[id.index()];

    // Order: clip is in parent-of-panel space (pre-transform); transform
    // applies inside the clip and only to children. The panel's own
    // background paints under the clip but BEFORE the transform — matching
    // WPF's `RenderTransform` convention.
    //
    // Exception: for `ClipMode::Rounded`, chrome paints BEFORE the clip
    // is pushed. The rounded mask is inset by the stroke width so
    // children can't overpaint the panel's stroke; that means chrome
    // pixels at the stroke region sit outside the mask. If chrome
    // painted under the mask too, its stroke would also be discarded.
    // Painting chrome unmasked (it self-clips via the SDF) keeps the
    // stroke visible while children stay clipped to the inset
    // interior.
    let mode = tree.records.attrs()[id.index()].clip_mode();
    let clip = mode.is_clip();
    let chrome = tree.chrome_for(id).copied();

    let paints = damage_filter.is_none_or(|d| cascades.rows[id.index()].screen_rect.intersects(d));

    // Chrome paints BEFORE the clip is pushed. The clip rect is
    // deflated by the chrome's stroke width (so children don't paint
    // over the stroke), which means chrome's own stroke pixels would
    // also fall outside the deflated region and be clipped. Painting
    // chrome first leaves it unclipped (the panel's SDF self-clips
    // correctly), preserving the stroke ring.
    if paints
        && let Some(bg) = chrome
        && !bg.is_noop()
    {
        out.draw_rect(rect, bg.radius, bg.fill, bg.stroke);
    }

    if clip {
        // Inset the clip by the chrome's stroke width so children
        // clip inside the painted stroke ring. With no chrome (clip
        // set without a Surface — shouldn't happen post-`apply_to`),
        // inset is 0.
        let inset = chrome.and_then(|bg| bg.stroke).map_or(0.0, |s| s.width);
        let mask_rect = rect.deflated_by(Spacing::all(inset));
        match mode {
            ClipMode::Rect => out.push_clip(mask_rect),
            ClipMode::Rounded => {
                // Reduce each corner radius by `inset` so the mask
                // curve stays concentric with the painted stroke's
                // inner edge — both curves have center at
                // `(rect.min + paint.radius)`.
                let painted = chrome
                    .map(|bg| bg.radius)
                    .expect("ClipMode::Rounded without chrome — builder invariant violated");
                let mask_radius = Corners {
                    tl: (painted.tl - inset).max(0.0),
                    tr: (painted.tr - inset).max(0.0),
                    br: (painted.br - inset).max(0.0),
                    bl: (painted.bl - inset).max(0.0),
                };
                out.push_clip_rounded(mask_rect, mask_radius);
            }
            ClipMode::None => {}
        }
    }

    // Damage filter: skip leaf shape emission when this node's
    // *screen* rect (layout rect projected through ancestor
    // transforms via `cascades`) doesn't intersect the dirty region.
    // Damage rects in `damage_filter` are also screen-space, so the
    // comparison is consistent under arbitrary transform stacks.
    // Push/PopClip and Push/PopTransform are still emitted (above and
    // below) so scissor groups and child transforms stay coherent.
    // `None` filter ⇒ paint everything.
    //
    // Clip culling (skipping leaves outside the active ancestor clip)
    // intentionally does NOT live here: it would make cmd shape
    // depend on screen position, breaking the encode cache's
    // authoring-only key. The composer culls per-cmd at compose time.

    // Skip Push/PopTransform when the transform is identity — composing
    // identity is a no-op, so emitting the pair just wastes two cmd
    // slots and a `transform_stack` push/pop in the composer.
    let transform = tree
        .read_extras(id)
        .transform
        .filter(|t| *t != TranslateScale::IDENTITY);

    // Walk this node's slice of the kinds stream, depth 0 only:
    //   - `Shape` at depth 0 → emit at owner rect (or its sub-rect)
    //   - `NodeEnter` at depth 0 → recurse for that direct child,
    //     bracketed by the owner's pan transform if non-identity, then
    //     skip the pos cursor past the child's full subtree.
    // Shapes always paint *outside* the owner's pan so they stay
    // anchored to the owner regardless of scroll offset; transform is
    // pushed/popped per child accordingly.
    let r = tree.records.kinds()[id.index()].range();
    let body_start = r.start + 1;
    let body_end = r.end - 1;
    let mut pos = body_start;
    let mut children = tree.children(id).map(|c| c.id);
    while pos < body_end {
        match tree.kinds[pos] {
            TreeOp::Shape => {
                if paints {
                    emit_one_shape(tree, layout, id, rect, &tree.shapes[*shape_cursor], out);
                }
                *shape_cursor += 1;
                pos += 1;
            }
            TreeOp::NodeEnter => {
                let child = children
                    .next()
                    .expect("kinds NodeEnter at depth 0 but children() exhausted");
                if let Some(t) = transform {
                    out.push_transform(t);
                }
                encode_node(
                    tree,
                    layout,
                    cascades,
                    damage_filter,
                    cache,
                    child,
                    shape_cursor,
                    out,
                );
                if transform.is_some() {
                    out.pop_transform();
                }
                pos = tree.records.kinds()[child.index()].range().end;
            }
            TreeOp::NodeExit => unreachable!(
                "owner's matching NodeExit excluded by body_end; nested exits skipped via node_kinds.range().end"
            ),
        }
    }

    if clip {
        out.pop_clip();
    }

    if let Some(p) = cache_pending {
        out.push_exit_subtree(p.enter_patch);
        let cmd_hi = out.kinds.len() as u32;
        let data_hi = out.data.len() as u32;
        cache.write_subtree(
            p.wid,
            p.subtree_hash,
            p.avail,
            out,
            (p.cmd_lo..cmd_hi).into(),
            (p.data_lo..data_hi).into(),
            layout.rect[id.index()].min,
        );
    }
}

/// Position a text run's bounding box inside a leaf's arranged rect per
/// `align`. Returns a rect with `min` shifted by the alignment offset
/// and `size` shrunk to the measured text bbox — composer takes
/// `min` as the glyph origin and `size` as the clip bounds. Glyphs
/// don't stretch, so `Auto`/`Stretch` collapse to start (top-left)
/// — matches `place_axis`'s behavior for non-stretchable content.
fn align_text_in(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
}

pub(crate) mod cache;

#[cfg(test)]
mod tests;
