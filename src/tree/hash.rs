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
use crate::element::{LayoutCore, LayoutMode, PaintAttrs, PaintCore};
use crate::primitives::{
    Align, Color, Corners, GridCell, Justify, Sense, Size, Sizes, Sizing, Spacing, Stroke, Track,
    Visibility,
};
use crate::shape::{Shape, TextWrap};
use glam::Vec2;
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

#[inline]
fn hash_f32(h: &mut impl Hasher, v: f32) {
    h.write_u32(v.to_bits());
}

#[inline]
fn hash_vec2(h: &mut impl Hasher, v: Vec2) {
    hash_f32(h, v.x);
    hash_f32(h, v.y);
}

#[inline]
fn hash_size(h: &mut impl Hasher, s: Size) {
    hash_f32(h, s.w);
    hash_f32(h, s.h);
}

#[inline]
fn hash_spacing(h: &mut impl Hasher, s: Spacing) {
    hash_f32(h, s.left);
    hash_f32(h, s.top);
    hash_f32(h, s.right);
    hash_f32(h, s.bottom);
}

#[inline]
fn hash_color(h: &mut impl Hasher, c: Color) {
    hash_f32(h, c.r);
    hash_f32(h, c.g);
    hash_f32(h, c.b);
    hash_f32(h, c.a);
}

#[inline]
fn hash_corners(h: &mut impl Hasher, c: Corners) {
    hash_f32(h, c.tl);
    hash_f32(h, c.tr);
    hash_f32(h, c.br);
    hash_f32(h, c.bl);
}

#[inline]
fn hash_stroke(h: &mut impl Hasher, s: Stroke) {
    hash_f32(h, s.width);
    hash_color(h, s.color);
}

#[inline]
fn hash_sizing(h: &mut impl Hasher, s: Sizing) {
    match s {
        Sizing::Fixed(v) => {
            h.write_u8(0);
            hash_f32(h, v);
        }
        Sizing::Hug => {
            h.write_u8(1);
        }
        Sizing::Fill(w) => {
            h.write_u8(2);
            hash_f32(h, w);
        }
    }
}

#[inline]
fn hash_sizes(h: &mut impl Hasher, s: Sizes) {
    hash_sizing(h, s.w);
    hash_sizing(h, s.h);
}

#[inline]
fn hash_align(h: &mut impl Hasher, a: Align) {
    // Align is bit-packed into a u8 (3+3 bits). Hash the byte directly.
    a.hash(h);
}

#[inline]
fn hash_visibility(h: &mut impl Hasher, v: Visibility) {
    v.hash(h);
}

#[inline]
fn hash_justify(h: &mut impl Hasher, j: Justify) {
    j.hash(h);
}

#[inline]
fn hash_sense(h: &mut impl Hasher, s: Sense) {
    s.hash(h);
}

#[inline]
fn hash_grid_cell(h: &mut impl Hasher, c: GridCell) {
    h.write_u16(c.row);
    h.write_u16(c.col);
    h.write_u16(c.row_span);
    h.write_u16(c.col_span);
}

#[inline]
fn hash_layout_mode(h: &mut impl Hasher, m: LayoutMode) {
    match m {
        LayoutMode::Leaf => h.write_u8(0),
        LayoutMode::HStack => h.write_u8(1),
        LayoutMode::VStack => h.write_u8(2),
        LayoutMode::WrapHStack => h.write_u8(3),
        LayoutMode::WrapVStack => h.write_u8(4),
        LayoutMode::ZStack => h.write_u8(5),
        LayoutMode::Canvas => h.write_u8(6),
        LayoutMode::Grid(idx) => {
            h.write_u8(7);
            h.write_u16(idx);
        }
    }
}

fn hash_layout_core(h: &mut impl Hasher, l: &LayoutCore) {
    hash_layout_mode(h, l.mode);
    hash_sizes(h, l.size);
    hash_spacing(h, l.padding);
    hash_spacing(h, l.margin);
    hash_align(h, l.align);
    hash_visibility(h, l.visibility);
}

fn hash_paint_attrs(h: &mut impl Hasher, a: PaintAttrs) {
    // PaintAttrs is a packed byte (sense + disabled + clip). Hash via
    // accessors so the byte layout is decoupled from the hash format —
    // if someone refactors the packing, the hash spec doesn't shift.
    hash_sense(h, a.sense());
    h.write_u8(a.is_disabled() as u8);
    h.write_u8(a.is_clip() as u8);
}

fn hash_paint_core(h: &mut impl Hasher, p: PaintCore) {
    hash_paint_attrs(h, p.attrs);
    // `extras: Option<u16>` is just a side-table index — its presence
    // matters (Some vs None), but not its numeric value across frames
    // since `node_extras` is rebuilt every frame. The extras *contents*
    // are hashed separately by `hash_node_extras`.
    h.write_u8(p.extras.is_some() as u8);
}

fn hash_node_extras(h: &mut impl Hasher, e: &crate::element::ElementExtras) {
    // `transform` is intentionally omitted: it doesn't affect this
    // node's own paint (the encoder draws the node at its layout rect
    // *before* `PushTransform`; the transform composes into
    // descendants' screen rects via `Cascades`). A parent transform
    // change shows up as descendant screen-rect diffs in
    // `Damage::compute`, which is the right granularity — the parent
    // itself doesn't need a fresh paint.
    hash_vec2(h, e.position);
    hash_grid_cell(h, e.grid);
    hash_size(h, e.min_size);
    hash_size(h, e.max_size);
    hash_f32(h, e.gap);
    hash_f32(h, e.line_gap);
    hash_justify(h, e.justify);
    hash_align(h, e.child_align);
}

#[inline]
fn hash_text_wrap(h: &mut impl Hasher, w: TextWrap) {
    w.hash(h);
}

fn hash_shape(h: &mut impl Hasher, shape: &Shape) {
    match shape {
        Shape::RoundedRect {
            radius,
            fill,
            stroke,
        } => {
            h.write_u8(0);
            hash_corners(h, *radius);
            hash_color(h, *fill);
            h.write_u8(stroke.is_some() as u8);
            if let Some(s) = stroke {
                hash_stroke(h, *s);
            }
        }
        Shape::Line { a, b, width, color } => {
            h.write_u8(1);
            hash_vec2(h, *a);
            hash_vec2(h, *b);
            hash_f32(h, *width);
            hash_color(h, *color);
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
            hash_color(h, *color);
            hash_f32(h, *font_size_px);
            hash_text_wrap(h, *wrap);
            hash_align(h, *align);
        }
    }
}

fn hash_track(h: &mut impl Hasher, t: &Track) {
    hash_sizing(h, t.size);
    hash_f32(h, t.min);
    hash_f32(h, t.max);
}

fn hash_grid_def(h: &mut impl Hasher, def: &GridDef) {
    h.write_u32(def.rows.len() as u32);
    for t in def.rows.iter() {
        hash_track(h, t);
    }
    h.write_u32(def.cols.len() as u32);
    for t in def.cols.iter() {
        hash_track(h, t);
    }
    hash_f32(h, def.row_gap);
    hash_f32(h, def.col_gap);
}

/// Compute the authoring hash for one node. Read-only over the tree —
/// pure function of (LayoutCore, PaintCore, ElementExtras, shapes,
/// optional GridDef) at this `NodeId`.
pub(super) fn compute_node_hash(
    layout: &LayoutCore,
    paint: PaintCore,
    extras: Option<&crate::element::ElementExtras>,
    shapes: &[Shape],
    grid_def: Option<&GridDef>,
) -> u64 {
    let mut h = FxHasher::default();
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
    h.finish()
}
