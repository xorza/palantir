use super::cmd_buffer::{EnterPatch, RenderCmdBuffer};
use crate::layout::types::{align::Align, align::HAlign, align::VAlign};
use crate::layout::{cache::AvailableKey, result::LayoutResult};
use crate::primitives::{
    corners::Corners, rect::Rect, size::Size, spacing::Spacing, transform::TranslateScale,
};
use crate::shape::Shape;
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree, node_hash::NodeHash};
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
/// The cache is **only consulted when `damage_filter.is_none()`** — see
/// `cache::EncodeCache` for the cascade-not-in-key argument.
/// Damage-filtered frames bypass the cache entirely; full-repaint frames
/// (resize, theme, first frame) and `damage_filter=None` paths get the
/// win.
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
            encode_node(
                tree,
                layout,
                cascades,
                damage_filter,
                &mut self.cache,
                root,
                &mut self.cmds,
            );
        }
        &self.cmds
    }
}

fn encode_node(
    tree: &Tree,
    layout: &LayoutResult,
    cascades: &CascadeResult,
    damage_filter: Option<Rect>,
    cache: &mut EncodeCache,
    id: NodeId,
    out: &mut RenderCmdBuffer,
) {
    // Hidden / Collapsed: paint nothing for this node or its subtree.
    // The cascade table already composed self + ancestors; recursing skips
    // the whole subtree because we early-return at the top of every node.
    if cascades.rows[id.index()].invisible {
        return;
    }

    // Cross-frame subtree-skip cache (Phase 3). Only consulted on
    // full-paint frames (`damage_filter.is_none()`) — the descendant
    // is_invisible / clip / transform reads inside this subtree are
    // captured by `subtree_hash`; `screen_rect` is the only cascade
    // input that would force re-keying, and it's read only when
    // `damage_filter.is_some()`. See `cache::EncodeCache`.
    //
    // Damage-filtered frames neither hit nor refresh the cache: a
    // partial repaint paints a strict subset of the tree, so writing
    // back would lie about the snapshot covering the full subtree.
    // Cache snapshot age is therefore bounded by the *last full-paint*
    // frame, not the last frame.
    let subtree_size = tree.subtree_end[id.index()] - id.index() as u32;
    let cache_eligible = damage_filter.is_none() && subtree_size > TINY_SUBTREE_THRESHOLD;
    let cache_key = if cache_eligible {
        layout
            .available_q(id)
            .map(|avail| (tree.widget_ids[id.index()], tree.subtree_hash(id), avail))
    } else {
        None
    };

    if let Some((wid, hash, avail)) = cache_key
        && cache.try_replay(wid, hash, avail, out, layout.rect[id.index()].min)
    {
        return;
    }

    // Bracket cache-eligible subtrees with `EnterSubtree`/`ExitSubtree`
    // markers. The markers go *inside* the snapshot range (cmd_lo is
    // captured before `push_enter_subtree` so the open cmd is at index
    // `cmd_lo`; the close is the last cmd in the range). Composer
    // reads `EnterSubtree` to attempt a splice (fast-forwarding past
    // the matching `ExitSubtree` on a hit) and uses `ExitSubtree` to
    // write the snapshot back on a miss.
    let cache_pending = if let Some((wid, subtree_hash, avail)) = cache_key {
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
    let mode = tree.paint[id.index()].attrs.clip_mode();
    let clip = mode.is_clip();
    // For both Rect and Rounded, chrome paints BEFORE the clip is
    // pushed: the clip rect is deflated by the panel's stroke width
    // (so children don't paint over the stroke), which means chrome's
    // own stroke pixels would also fall outside the deflated region
    // and be clipped. Painting chrome first leaves it unclipped (the
    // panel's SDF self-clips correctly), preserving the stroke ring.
    let chrome_before_clip = clip;

    let paints = damage_filter.is_none_or(|d| cascades.rows[id.index()].screen_rect.intersects(d));

    if chrome_before_clip && paints {
        emit_background_shapes(tree, layout, id, rect, out);
    }

    if clip {
        // Builder (`Surface::apply_clip`) guarantees a mask is stamped
        // whenever clip mode survives as `Rect` or `Rounded`. Deflate
        // the layout rect by `mask.inset` (= painted stroke width) so
        // children clip inside the stroke ring.
        let mask = tree
            .read_extras(id)
            .clip_mask
            .expect("clip != None without clip_mask — builder invariant violated");
        let mask_rect = rect.deflated_by(Spacing::all(mask.inset));
        match mask.radius {
            None => out.push_clip(mask_rect),
            Some(r) => {
                // Reduce each corner radius by `inset` so the mask
                // curve stays concentric with the painted stroke's
                // inner edge — both curves have center at
                // `(rect.min + paint.radius)`. Inflating instead
                // would offset the curve center inward and produce a
                // visible notch where the mismatched curves meet.
                let mask_radius = Corners {
                    tl: (r.tl - mask.inset).max(0.0),
                    tr: (r.tr - mask.inset).max(0.0),
                    br: (r.br - mask.inset).max(0.0),
                    bl: (r.bl - mask.inset).max(0.0),
                };
                out.push_clip_rounded(mask_rect, mask_radius);
            }
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

    // Two-phase shape emission per node:
    //   - Background shapes (RoundedRect, Text) paint BEFORE children
    //     under the owner's clip but pre-transform — backgrounds and
    //     text labels live "behind" descendants.
    //   - Overlay shapes (Overlay) paint AFTER children, still
    //     under the clip and untransformed — used for sub-rect
    //     overlays like scrollbar tracks/thumbs that must sit on top
    //     of (and not pan with) the content.
    if paints && !chrome_before_clip {
        emit_background_shapes(tree, layout, id, rect, out);
    }

    // Skip Push/PopTransform when the transform is identity — composing
    // identity is a no-op, so emitting the pair just wastes two cmd
    // slots and a `transform_stack` push/pop in the composer.
    let transform = tree
        .read_extras(id)
        .transform
        .filter(|t| *t != TranslateScale::IDENTITY);
    if let Some(t) = transform {
        out.push_transform(t);
    }

    for child in tree.children(id) {
        encode_node(tree, layout, cascades, damage_filter, cache, child, out);
    }

    if transform.is_some() {
        out.pop_transform();
    }

    // Overlay phase: Overlay shapes paint on top of children
    // but still under the owner's clip and untransformed by the
    // owner's pan. This is what scrollbar tracks/thumbs need — they
    // sit inside the viewport's clip but on top of (and not panning
    // with) the content.
    if paints {
        for shape in tree.shapes.slice_of(id.index()) {
            if let Shape::Overlay {
                rect: sub,
                radius,
                fill,
            } = shape
            {
                let r = Rect {
                    min: rect.min + sub.min,
                    size: sub.size,
                };
                out.draw_rect(r, *radius, *fill, None);
            }
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

/// Emit a node's "background phase" shapes (panel chrome + text
/// runs). Called from `encode_node` either before the clip push
/// (rounded-clip mode, so chrome stays unmasked) or after (rect /
/// no-clip, WPF-style chrome-under-clip).
fn emit_background_shapes(
    tree: &Tree,
    layout: &LayoutResult,
    id: NodeId,
    rect: Rect,
    out: &mut RenderCmdBuffer,
) {
    for shape in tree.shapes.slice_of(id.index()) {
        match shape {
            Shape::RoundedRect {
                radius,
                fill,
                stroke,
            } => {
                out.draw_rect(rect, *radius, *fill, *stroke);
            }
            Shape::Text { color, align, .. } => {
                let Some(shaped) = layout.text_shapes[id.index()] else {
                    continue;
                };
                if shaped.key.is_invalid() {
                    tracing::trace!(?shape, "encoder: dropping text with invalid key");
                    continue;
                }
                let inner = rect.deflated_by(tree.layout[id.index()].padding);
                out.draw_text(
                    align_text_in(inner, shaped.measured, *align),
                    *color,
                    shaped.key,
                );
            }
            // Overlay phase emits these after children — see encode_node.
            Shape::Overlay { .. } => {}
            Shape::Line { .. } => {
                tracing::trace!(?shape, "encoder: dropping unsupported shape");
            }
        }
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
