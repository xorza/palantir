//! Per-node authoring-hash computation. Walks every field that affects
//! rendering output and folds it into a 64-bit `FxHash`. Captures the
//! "what the user typed" snapshot for a node — the inputs, not the
//! derived layout output (`rect`, `desired`).
//!
//! Step 1 of the damage-rect rendering plan (see `docs/damage-rendering.md`).
//! Currently *computed but not consumed*: the hashes ship as a column on
//! `Tree` so future steps (persistent prev-map, dirty-set) can read them.
//!
//! All `f32` fields hash via `to_bits()` — exact bit equality, not
//! `==`-equality, so `0.0` vs `-0.0` hash differently (over-eager dirty
//! marking, fine for our use). NaN handling is consistent for the same
//! NaN bit pattern; UI authoring shouldn't produce NaN anyway (asserts
//! in builders enforce non-negative sizes etc.).

use super::GridDef;
use crate::layout::types::{sizing::Sizes, sizing::Sizing, track::Track};
use crate::shape::Shape;
use crate::tree::element::{ElementExtras, LayoutCore, LayoutMode, PaintCore};
use rustc_hash::FxHasher;
use std::hash::Hash;
use std::hash::Hasher as _;

/// Authoring-hash newtype. A 64-bit `FxHash` over the inputs that
/// affect rendering output for one node — *not* the derived layout
/// output. Wrapping `u64` rather than passing it bare prevents
/// confusion with `WidgetId` / other 64-bit handles in signatures
/// like `shape_unbounded(wid: WidgetId, hash: NodeHash, …)`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NodeHash(u64);

impl NodeHash {
    /// Sentinel returned by `Tree::node_hash` before
    /// `compute_hashes` runs. Distinguishable from any real hash only
    /// probabilistically (collisions are 2⁻⁶⁴), but adequate as an
    /// "uninitialized" marker.
    pub(crate) const UNCOMPUTED: Self = Self(0);

    /// Raw 64-bit hash value. Exposed so `Tree::compute_hashes` can
    /// fold per-node hashes into the subtree-hash rollup without
    /// reaching into private fields.
    #[inline]
    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }

    /// Construct a `NodeHash` from a raw `u64`. Same use-case as
    /// [`Self::as_u64`].
    #[inline]
    pub(crate) fn from_u64(v: u64) -> Self {
        Self(v)
    }
}

/// `FxHasher` wrapper that adds `pod()` for whole-value byte writes.
/// Use this everywhere we'd otherwise reach for `FxHasher::default()`
/// directly so the `pod` shortcut and `std::hash::Hasher` trait are
/// always in scope at the same time.
///
/// Implements `std::hash::Hasher` so `value.hash(&mut h)` and
/// `h.write_u8(...)` etc. work unchanged.
pub(crate) struct Hasher(FxHasher);

impl Hasher {
    #[inline]
    pub(crate) fn new() -> Self {
        Self(FxHasher::default())
    }

    /// Hash a value as its raw bytes in one `Hasher::write` call. The
    /// `NoUninit` bound proves at compile time that `T` has no padding
    /// so `bytes_of` is sound.
    ///
    /// Why this is faster than per-field writes: `FxHasher::write(&[u8])`
    /// consumes 8 bytes per loop iteration and amortizes the
    /// rotate/multiply/xor cost across the whole slice. Replacing
    /// N×`write_u32`/`write_u16` calls with one `write` cuts per-call
    /// overhead and lets the compiler keep more state in registers.
    #[inline]
    pub(crate) fn pod<T: bytemuck::NoUninit>(&mut self, v: &T) {
        self.0.write(bytemuck::bytes_of(v));
    }
}

impl std::hash::Hasher for Hasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0.finish()
    }
}

/// `Sizing` is a tagged union with niche-uninit padding in its inactive
/// variant — `pod` would hash junk bytes. Encode as a deterministic
/// `tag:u8 + value:f32` instead. Inlined for the two `Sizes` axes.
#[inline]
fn hash_sizing(h: &mut Hasher, s: Sizing) {
    let (tag, v) = match s {
        Sizing::Fixed(v) => (0u8, v),
        Sizing::Hug => (1, 0.0),
        Sizing::Fill(w) => (2, w),
    };
    h.write_u8(tag);
    h.write_u32(v.to_bits());
}

#[inline]
fn hash_sizes(h: &mut Hasher, s: Sizes) {
    hash_sizing(h, s.w);
    hash_sizing(h, s.h);
}

/// Same shape as `hash_sizing`: tagged union, inactive payload bytes are
/// uninit, so explicit tag+payload encoding rather than `pod`. Packs the
/// 1-byte tag + optional 2-byte payload into a single 32-bit write
/// (high 16 bits zero for non-Grid variants).
#[inline]
fn hash_layout_mode(h: &mut Hasher, m: LayoutMode) {
    let packed: u32 = match m {
        LayoutMode::Leaf => 0,
        LayoutMode::HStack => 1,
        LayoutMode::VStack => 2,
        LayoutMode::WrapHStack => 3,
        LayoutMode::WrapVStack => 4,
        LayoutMode::ZStack => 5,
        LayoutMode::Canvas => 6,
        LayoutMode::Grid(idx) => 7 | ((idx as u32) << 16),
    };
    h.write_u32(packed);
}

#[inline]
fn hash_layout_core(h: &mut Hasher, l: &LayoutCore) {
    hash_layout_mode(h, l.mode);
    hash_sizes(h, l.size);
    // padding + margin: two `Spacing`s (4 f32 each = 32 contiguous bytes).
    h.pod(&[l.padding, l.margin]);
    // Pack Align (u8) + Visibility (u8 discriminant) into one u16 write.
    h.write_u16(((l.visibility as u8 as u16) << 8) | l.align.raw() as u16);
}

#[inline]
fn hash_paint_core(h: &mut Hasher, p: PaintCore) {
    // PaintAttrs sense (3 bits) + disabled + clip + extras-presence — all
    // small flags. Pack into one u16 instead of four byte writes.
    let a = p.attrs;
    let packed = (a.sense() as u16)
        | ((a.is_disabled() as u16) << 8)
        | ((a.is_clip() as u16) << 9)
        | ((p.extras.is_some() as u16) << 10);
    // `extras: Option<u16>` is a side-table index — only its presence
    // matters across frames (the table is rebuilt each frame); contents
    // are hashed separately by `hash_node_extras`.
    h.write_u16(packed);
}

#[inline]
fn hash_node_extras(h: &mut Hasher, e: &ElementExtras) {
    // `transform` is intentionally omitted: it doesn't affect this
    // node's own paint (the encoder draws the node at its layout rect
    // *before* `PushTransform`; the transform composes into
    // descendants' screen rects via `Cascades`). A parent transform
    // change shows up as descendant screen-rect diffs in
    // `Damage::compute`, which is the right granularity.
    h.pod(&e.position);
    h.pod(&e.grid);
    h.pod(&[e.min_size, e.max_size]);
    h.pod(&[e.gap, e.line_gap]);
    h.write_u16(((e.child_align.raw() as u16) << 8) | e.justify as u8 as u16);
}

#[inline]
fn hash_shape(h: &mut Hasher, shape: &Shape) {
    match shape {
        Shape::RoundedRect {
            radius,
            fill,
            stroke,
        } => {
            h.write_u8(0);
            h.pod(radius);
            h.pod(fill);
            match stroke {
                None => h.write_u8(0),
                Some(s) => {
                    h.write_u8(1);
                    h.pod(s);
                }
            }
        }
        Shape::Line { a, b, width, color } => {
            h.write_u8(1);
            h.pod(a);
            h.pod(b);
            h.write_u32(width.to_bits());
            h.pod(color);
        }
        Shape::Text {
            text,
            color,
            font_size_px,
            wrap,
            align,
        } => {
            h.write_u8(2);
            text.hash(h);
            h.pod(color);
            h.write_u32(font_size_px.to_bits());
            h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
        }
    }
}

#[inline]
fn hash_track(h: &mut Hasher, t: &Track) {
    hash_sizing(h, t.size);
    h.write_u32(t.min.to_bits());
    h.write_u32(t.max.to_bits());
}

#[inline]
fn hash_grid_def(h: &mut Hasher, def: &GridDef) {
    h.write_u32(def.rows.len() as u32);
    for t in def.rows.iter() {
        hash_track(h, t);
    }
    h.write_u32(def.cols.len() as u32);
    for t in def.cols.iter() {
        hash_track(h, t);
    }
    h.write_u32(def.row_gap.to_bits());
    h.write_u32(def.col_gap.to_bits());
}

/// Compute the authoring hash for one node. Read-only over the tree —
/// pure function of (LayoutCore, PaintCore, ElementExtras, shapes,
/// optional GridDef) at this `NodeId`.
#[inline]
pub(crate) fn compute_node_hash(
    layout: &LayoutCore,
    paint: PaintCore,
    extras: Option<&ElementExtras>,
    shapes: &[Shape],
    grid_def: Option<&GridDef>,
) -> NodeHash {
    let mut h = Hasher::new();
    hash_layout_core(&mut h, layout);
    hash_paint_core(&mut h, paint);
    if let Some(e) = extras {
        hash_node_extras(&mut h, e);
    }
    h.write_u32(shapes.len() as u32);
    for s in shapes {
        hash_shape(&mut h, s);
    }
    if let Some(def) = grid_def {
        hash_grid_def(&mut h, def);
    }
    NodeHash(h.finish())
}
