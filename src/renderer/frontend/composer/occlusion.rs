//! CPU occlusion pruning: drop quads in a scissor group that are fully
//! covered by a later opaque quad in the same group. Pure prune — no
//! pipeline / shader changes. See `docs/roadmap/occlusion-pruning.md`.

use crate::primitives::rect::Rect;
use crate::renderer::render_buffer::RenderBuffer;
use glam::Vec2;

/// One opaque occluder in the in-flight group. See [`OcclusionPruner`]
/// for the cover-rect contract.
#[derive(Clone, Copy)]
struct Occluder {
    /// Index inside the in-flight group's quad slice
    /// (`out.quads[quads_cursor + idx]`).
    idx: u32,
    /// Largest axis-aligned rect with full opaque coverage. Used as the
    /// left-hand side of `Rect::contains_rect(occludee.painted)` in the
    /// prune sweep, where `painted = occludee.rect.inflated(stroke_width / 2)`.
    cover: Rect,
}

/// Per-group occlusion-prune scratch. Accumulates the solid-opaque
/// occluders pushed into the in-flight group's quad slice, then drops
/// every earlier quad those occluders fully cover.
///
/// Each [`Occluder`] pairs the quad's slice-relative index (for
/// "drawn-on-top" ordering — only indices `> i` can occlude quad `i`)
/// with its **cover rect**: the largest axis-aligned rect guaranteed to
/// receive full opaque coverage. For sharp-cornered quads
/// `cover == Quad.rect`; for rounded quads, `cover` is `Quad.rect`
/// deflated per-side by `max(adjacent_radii) * (1 − 1/√2)` (the
/// inscribed-square offset of a corner arc).
#[derive(Default)]
pub(super) struct OcclusionPruner {
    /// Solid-opaque no-stroke occluders in the in-flight group, in push
    /// order (ascending `idx`).
    opaque_in_group: Vec<Occluder>,
    /// Indices (relative to the group's quad cursor) marked for removal
    /// by the prune sweep. Sorted ascending by construction.
    drop_indices: Vec<u32>,
    /// Prefix-max of `cover.size` over the tail of `opaque_in_group`,
    /// built once per prune. `prefix_max_cover[i]` = elementwise max
    /// over `opaque_in_group[i..]`. Lets the prune sweep reject an
    /// occludee with one size compare when no later occluder is large
    /// enough to contain it — turns the common "nested panels, child
    /// smaller than parent" case from O(N·K) into O(N + K).
    prefix_max_cover: Vec<Vec2>,
}

impl OcclusionPruner {
    /// Reset all scratch — called at each group flush and at compose
    /// start.
    pub(super) fn clear(&mut self) {
        self.opaque_in_group.clear();
        self.drop_indices.clear();
        self.prefix_max_cover.clear();
    }

    /// Record a solid-opaque quad's cover rect at its group-slice index
    /// `idx` so the prune sweep can drop earlier quads contained in it.
    pub(super) fn record_opaque(&mut self, idx: u32, cover: Rect) {
        self.opaque_in_group.push(Occluder { idx, cover });
    }

    /// Drop quads in the in-flight group (`out.quads[quads_cursor..]`)
    /// that are fully covered by a later opaque quad in the same group.
    ///
    /// Preconditions:
    /// - `out.quads[quads_cursor..]` is the in-flight group's contiguous
    ///   slice (composer's flush boundary contract).
    /// - `opaque_in_group` holds an entry for every solid-opaque quad
    ///   pushed into the slice, in push order (ascending `idx`). Stroke
    ///   status is irrelevant on the occluder side (fill alone covers the
    ///   interior).
    ///
    /// Behaviour:
    /// - For each quad at slice index `i`, compute its painted extent as
    ///   `q.rect.inflated(q.stroke_width / 2)` (centred strokes spill
    ///   outward; non-stroked inflate by zero). Drop it if some occluder
    ///   with `idx > i` (drawn on top) has `cover.contains_rect(painted)`.
    /// - Shadows (`FillKind::is_shadow`) are never dropped — their visual
    ///   blur extends past the stored rect.
    /// - Compacts in place via copy-down; preserves survivor order.
    pub(super) fn prune(&mut self, out: &mut RenderBuffer, quads_cursor: u32) {
        let start = quads_cursor as usize;
        if out.quads.len() - start < 2 || self.opaque_in_group.is_empty() {
            return;
        }
        let slice = &out.quads[start..];
        let occs = self.opaque_in_group.as_slice();

        // Prefix-max of cover dimensions over the tail of occs. After
        // this loop, `prefix_max_cover[i]` is the elementwise max
        // `(w, h)` over `occs[i..]`. Used below as a one-comparison
        // reject: if the occludee's painted rect is wider or taller
        // than every remaining cover, no `contains_rect` can succeed.
        self.prefix_max_cover.clear();
        self.prefix_max_cover.resize(occs.len(), Vec2::ZERO);
        let mut acc = Vec2::ZERO;
        for (i, occ) in occs.iter().enumerate().rev() {
            acc = acc.max(Vec2::new(occ.cover.size.w, occ.cover.size.h));
            self.prefix_max_cover[i] = acc;
        }

        self.drop_indices.clear();
        // Cursor into `occs` advancing in lockstep with `i`: it's
        // always positioned at the first occluder with `idx > i`.
        // Since `i` and `occs[*].idx` are both monotonically
        // ascending, the cursor only moves forward across the outer
        // loop — total work is O(N + K), not O(N·K).
        let mut cursor = 0;
        for (i, q) in slice.iter().enumerate() {
            // Shadows paint past the stored rect by blur sigma (no
            // closed-form extent we can test cheaply) — never drop.
            if q.fill_kind.is_shadow() {
                continue;
            }
            while cursor < occs.len() && occs[cursor].idx as usize <= i {
                cursor += 1;
            }
            // No later occluder exists for this `i` — and since
            // subsequent `i` values need even later occluders, none
            // can be covered. Done.
            if cursor >= occs.len() {
                break;
            }
            // Centred strokes paint outside the rect by
            // `stroke_width / 2` on every edge. Inflate the
            // occludee's painted extent for the containment test;
            // non-stroked quads inflate by zero. Rounded under-quads
            // share their bounding rect with the painted region, so
            // no corner-specific handling needed on this side.
            let painted = q.rect.inflated(q.stroke_width * 0.5);
            // Cheap reject: no remaining cover is large enough to
            // contain `painted` on at least one axis. This catches
            // the dominant "nested panels, parent larger than every
            // descendant" pattern without touching the inner loop.
            let max = self.prefix_max_cover[cursor];
            if painted.size.w > max.x || painted.size.h > max.y {
                continue;
            }
            for occ in &occs[cursor..] {
                if occ.cover.contains_rect(painted) {
                    self.drop_indices.push(i as u32);
                    break;
                }
            }
        }
        if self.drop_indices.is_empty() {
            return;
        }
        // Compact in place: walk forward, copy survivors down. The
        // drop list is sorted ascending by construction.
        let mut drop_iter = self.drop_indices.iter().copied().peekable();
        let mut write = start;
        for read in start..out.quads.len() {
            let rel = (read - start) as u32;
            if drop_iter.peek().copied() == Some(rel) {
                drop_iter.next();
                continue;
            }
            if read != write {
                out.quads[write] = out.quads[read];
            }
            write += 1;
        }
        out.quads.truncate(write);
    }
}
