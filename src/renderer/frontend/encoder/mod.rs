use super::cmd_buffer::RenderCmdBuffer;
use crate::cascade::CascadeResult;
use crate::layout::{cache::AvailableKey, result::LayoutResult};
use crate::primitives::{
    align::Align, align::HAlign, align::VAlign, rect::Rect, size::Size, span::Span,
    transform::TranslateScale, widget_id::WidgetId,
};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree, hash::NodeHash};
use cache::EncodeCache;

/// Bookkeeping captured before recursing so we can write the cached
/// subtree back after children have appended their cmds. `cmd_lo` /
/// `data_lo` snapshot `out`'s arena lengths at entry; the hi ends are
/// read after recursion to form the subtree's spans.
struct CachePending {
    wid: WidgetId,
    hash: NodeHash,
    avail: AvailableKey,
    cmd_lo: u32,
    data_lo: u32,
}

/// Skip cache lookup + write for subtrees of `<=` this many nodes.
/// A 1-node subtree's encode work (one `draw_rect` or `draw_text`) is
/// cheaper than the hashmap miss + insert it would replace. The win
/// shows on cold / forced-miss frames where every parent miss falls
/// through to per-leaf cache I/O.
const TINY_SUBTREE_THRESHOLD: u32 = 1;

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

    /// Drop cache entries for `WidgetId`s that vanished this frame.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.cache.sweep_removed(removed);
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
        && let Some(hit) = cache.try_lookup(wid, hash, avail)
    {
        out.extend_from_cached(hit.kinds, hit.starts, hit.data, layout.rect(id).min);
        return;
    }

    let cache_pending = cache_key.map(|(wid, hash, avail)| CachePending {
        wid,
        hash,
        avail,
        cmd_lo: out.kinds.len() as u32,
        data_lo: out.data.len() as u32,
    });

    let rect = layout.rect(id);

    // Order: clip is in parent-of-panel space (pre-transform); transform
    // applies inside the clip and only to children. The panel's own
    // background paints under the clip but BEFORE the transform — matching
    // WPF's `RenderTransform` convention.
    let clip = tree.paint(id).attrs.is_clip();
    if clip {
        out.push_clip(rect);
    }

    // Damage filter: skip leaf shape emission when this node's
    // *screen* rect (layout rect projected through ancestor
    // transforms via `cascades`) doesn't intersect the dirty region.
    // Damage rects in `damage_filter` are also screen-space, so the
    // comparison is consistent under arbitrary transform stacks.
    // Push/PopClip and Push/PopTransform are still emitted (above and
    // below) so scissor groups and child transforms stay coherent.
    // `None` filter ⇒ paint everything.
    let paints = damage_filter.is_none_or(|d| cascades.rows[id.index()].screen_rect.intersects(d));

    if paints {
        for shape in tree.shapes_of(id) {
            match shape {
                Shape::RoundedRect {
                    radius,
                    fill,
                    stroke,
                } => {
                    out.draw_rect(rect, *radius, *fill, *stroke);
                }
                Shape::Text { color, align, .. } => {
                    // Shaping happened in measure; the resulting buffer key is
                    // on `LayoutResult.text_shapes`. Missing entry means no
                    // shaper was installed (mono fallback) or the run was empty
                    // — drop in either case.
                    let Some(shaped) = layout.text_shape(id) else {
                        continue;
                    };
                    if shaped.key.is_invalid() {
                        tracing::trace!(?shape, "encoder: dropping text with invalid key");
                        continue;
                    }
                    // `layout.rect(id)` is padding-inclusive (the rendered
                    // rect that DrawRect paints). Text aligns within the
                    // padding-deflated content area so e.g. `Button.padding(8)`
                    // insets the label visually.
                    let inner = rect.deflated_by(tree.layout(id).padding);
                    out.draw_text(
                        align_text_in(inner, shaped.measured, *align),
                        *color,
                        shaped.key,
                    );
                }
                // No backend support for these yet — drop with a trace so they're
                // not silently invisible.
                Shape::Line { .. } => {
                    tracing::trace!(?shape, "encoder: dropping unsupported shape");
                }
            }
        }
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
    if clip {
        out.pop_clip();
    }

    if let Some(p) = cache_pending {
        let cmd_hi = out.kinds.len() as u32;
        let data_hi = out.data.len() as u32;
        cache.write_subtree(
            p.wid,
            p.hash,
            p.avail,
            out,
            Span::new(p.cmd_lo, cmd_hi - p.cmd_lo),
            Span::new(p.data_lo, data_hi - p.data_lo),
            layout.rect(id).min,
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
