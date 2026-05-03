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
use crate::element::{ElementExtras, LayoutCore, LayoutMode, PaintAttrs, PaintCore};
use crate::primitives::{
    Align, Color, Corners, GridCell, Justify, Size, Sizes, Sizing, Spacing, Stroke, Track,
};
use crate::shape::{Shape, TextWrap};
use glam::Vec2;
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

/// Authoring-hash newtype. A 64-bit `FxHash` over the inputs that
/// affect rendering output for one node — *not* the derived layout
/// output. Wrapping `u64` rather than passing it bare prevents
/// confusion with `WidgetId` / other 64-bit handles in signatures
/// like `shape_unbounded(wid: WidgetId, hash: NodeHash, …)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeHash(u64);

impl NodeHash {
    /// Sentinel returned by `Tree::node_hash` before
    /// `compute_hashes` runs. Distinguishable from any real hash only
    /// probabilistically (collisions are 2⁻⁶⁴), but adequate as an
    /// "uninitialized" marker.
    pub const UNCOMPUTED: Self = Self(0);
}

#[inline]
fn hash_f32(h: &mut impl Hasher, v: f32) {
    h.write_u32(v.to_bits());
}

/// Hash a value as its raw bytes in one `Hasher::write` call. Sound only
/// for `T` with no padding bytes — the caller is responsible for that.
/// Used here for f32-only structs (`Spacing`, `Color`, `Corners`,
/// `Stroke`, `Size`, `Vec2`), where homogeneous 4-byte alignment rules
/// out gaps.
///
/// Why this is faster: `FxHasher::write(&[u8])` consumes 8 bytes per
/// loop iteration and amortizes the rotate/multiply/xor cost across the
/// whole slice. Replacing N×`write_u32` calls with one `write` cuts the
/// per-call overhead and lets the compiler keep more state in registers.
#[inline]
fn hash_bytes_of<T>(h: &mut impl Hasher, v: &T) {
    let bytes =
        unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) };
    h.write(bytes);
}

#[inline]
fn hash_vec2(h: &mut impl Hasher, v: Vec2) {
    hash_bytes_of(h, &v);
}

#[inline]
fn hash_size(h: &mut impl Hasher, s: Size) {
    hash_bytes_of(h, &s);
}

#[inline]
fn hash_spacing(h: &mut impl Hasher, s: Spacing) {
    hash_bytes_of(h, &s);
}

#[inline]
fn hash_color(h: &mut impl Hasher, c: Color) {
    hash_bytes_of(h, &c);
}

#[inline]
fn hash_corners(h: &mut impl Hasher, c: Corners) {
    hash_bytes_of(h, &c);
}

#[inline]
fn hash_stroke(h: &mut impl Hasher, s: Stroke) {
    hash_bytes_of(h, &s);
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
    h.write_u8(a.raw());
}

#[inline]
fn hash_justify(h: &mut impl Hasher, j: Justify) {
    h.write_u8(j as u8);
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
    // Two homogeneous-f32 spacings, hashed as 32 contiguous bytes via two
    // calls (would be one write if Spacing pairs were stored adjacently).
    hash_spacing(h, l.padding);
    hash_spacing(h, l.margin);
    // Pack Align (u8) + Visibility (u8 discriminant) into one u16 write.
    // Visibility is a `#[repr]`-less enum, but its discriminant fits in a
    // byte (3 variants) and is stable within a build.
    let vis_byte = l.visibility as u8;
    h.write_u16(((vis_byte as u16) << 8) | l.align.raw() as u16);
}

fn hash_paint_attrs(h: &mut impl Hasher, a: PaintAttrs) {
    // PaintAttrs is a packed byte (sense + disabled + clip). Hash via
    // accessors so the byte layout is decoupled from the hash format —
    // if someone refactors the packing, the hash spec doesn't shift.
    let sense_byte: u8 = a.sense() as u8;
    let flags = (a.is_disabled() as u8) | ((a.is_clip() as u8) << 1);
    h.write_u16(((flags as u16) << 8) | sense_byte as u16);
}

fn hash_paint_core(h: &mut impl Hasher, p: PaintCore) {
    hash_paint_attrs(h, p.attrs);
    // `extras: Option<u16>` is just a side-table index — its presence
    // matters (Some vs None), but not its numeric value across frames
    // since `node_extras` is rebuilt every frame. The extras *contents*
    // are hashed separately by `hash_node_extras`.
    h.write_u8(p.extras.is_some() as u8);
}

fn hash_node_extras(h: &mut impl Hasher, e: &ElementExtras) {
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
    extras: Option<&ElementExtras>,
    shapes: &[Shape],
    grid_def: Option<&GridDef>,
) -> NodeHash {
    let mut h = FxHasher::default();
    #[cfg(test)]
    let mask = bench::HASH_MASK.with(|m| m.get());
    #[cfg(not(test))]
    let mask: u32 = u32::MAX;
    if mask & bench::M_LAYOUT != 0 {
        hash_layout_core(&mut h, layout);
    }
    if mask & bench::M_PAINT != 0 {
        hash_paint_core(&mut h, paint);
    }
    if mask & bench::M_EXTRAS != 0
        && let Some(e) = extras
    {
        hash_node_extras(&mut h, e);
    }
    if mask & bench::M_SHAPES != 0 {
        h.write_u32(shapes.len() as u32);
        for s in shapes {
            hash_shape(&mut h, s);
        }
    }
    if mask & bench::M_GRID != 0
        && let Some(def) = grid_def
    {
        hash_grid_def(&mut h, def);
    }
    NodeHash(h.finish())
}

pub(crate) mod bench {
    pub const M_LAYOUT: u32 = 1 << 0;
    pub const M_PAINT: u32 = 1 << 1;
    pub const M_EXTRAS: u32 = 1 << 2;
    pub const M_SHAPES: u32 = 1 << 3;
    pub const M_GRID: u32 = 1 << 4;
    #[allow(dead_code)]
    pub const M_ALL: u32 = M_LAYOUT | M_PAINT | M_EXTRAS | M_SHAPES | M_GRID;

    #[cfg(test)]
    thread_local! {
        pub static HASH_MASK: std::cell::Cell<u32> = const { std::cell::Cell::new(M_ALL) };
    }
}

#[cfg(test)]
mod bench_breakdown {
    //! Run with:
    //!   cargo test --release --lib tree::hash::bench_breakdown -- --ignored --nocapture
    use super::bench::*;
    use crate::primitives::Display;
    use crate::{Align, Button, Configure, Frame, Grid, Justify, Panel, Sizing, Text, Track, Ui};
    use glam::UVec2;
    use std::rc::Rc;
    use std::time::Instant;

    fn build_scene(ui: &mut Ui, scale: usize) {
        let sidebar_items = 5 * scale;
        let chat_messages = 2 * scale;
        let canvas_dots = 3 * scale;
        let prop_rows = 4 + scale;
        Panel::vstack()
            .gap(8.0)
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::hstack()
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .child_align(Align::CENTER)
                    .show(ui, |ui| {
                        Text::with_id("title", "Complex Layout Showcase")
                            .size_px(20.0)
                            .show(ui);
                        Frame::with_id("title-spacer")
                            .size((Sizing::FILL, Sizing::Fixed(1.0)))
                            .show(ui);
                        for i in 0..5 {
                            Button::with_id(("hdr", i))
                                .label(format!("Action {i}"))
                                .show(ui);
                        }
                    });
                Panel::hstack()
                    .gap(12.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .gap(4.0)
                            .padding(8.0)
                            .size((Sizing::Fixed(220.0), Sizing::FILL))
                            .show(ui, |ui| {
                                for i in 0..sidebar_items {
                                    Button::with_id(("side", i))
                                        .label(format!("Sidebar item {i}"))
                                        .size((Sizing::FILL, Sizing::Hug))
                                        .show(ui);
                                }
                                Frame::with_id("sb-divider")
                                    .size((Sizing::FILL, Sizing::Fixed(1.0)))
                                    .margin(4.0)
                                    .show(ui);
                                Panel::hstack()
                                    .gap(2.0)
                                    .justify(Justify::Center)
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .show(ui, |ui| {
                                        for i in 0..3 {
                                            Button::with_id(("sb-foot", i))
                                                .label(format!("F{i}"))
                                                .show(ui);
                                        }
                                    });
                            });
                        Panel::vstack()
                            .gap(10.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                let rows: Vec<Track> =
                                    (0..prop_rows).map(|_| Track::hug()).collect();
                                Grid::with_id("props")
                                    .cols(Rc::from([
                                        Track::hug().min(80.0),
                                        Track::fill(),
                                        Track::fixed(60.0),
                                    ]))
                                    .rows(Rc::<[Track]>::from(rows))
                                    .gap(6.0)
                                    .padding(4.0)
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .show(ui, |ui| {
                                        let labels =
                                            ["Name", "Description", "Author", "License"];
                                        let values = [
                                            "the quick brown fox jumps over the lazy dog",
                                            "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor",
                                            "Jane Doe and a long author name to force wrapping in narrow viewports",
                                            "MIT-or-Apache-2.0",
                                        ];
                                        for row in 0..prop_rows {
                                            let r = row as u16;
                                            Text::with_id(("plbl", row), labels[row % labels.len()])
                                                .size_px(14.0)
                                                .grid_cell((r, 0))
                                                .show(ui);
                                            Text::with_id(("pval", row), values[row % values.len()])
                                                .size_px(14.0)
                                                .wrapping()
                                                .grid_cell((r, 1))
                                                .show(ui);
                                            Button::with_id(("pact", row))
                                                .label("Edit")
                                                .grid_cell((r, 2))
                                                .show(ui);
                                        }
                                    });
                                for i in 0..chat_messages {
                                    Panel::hstack_with_id(("chat-row", i))
                                        .gap(8.0)
                                        .size((Sizing::FILL, Sizing::Hug))
                                        .show(ui, |ui| {
                                            Frame::with_id(("avatar", i))
                                                .size((
                                                    Sizing::Fixed(40.0),
                                                    Sizing::Fixed(40.0),
                                                ))
                                                .show(ui);
                                            Panel::vstack_with_id(("chat-text", i))
                                                .gap(2.0)
                                                .size((Sizing::FILL, Sizing::Hug))
                                                .show(ui, |ui| {
                                                    Text::with_id(
                                                        ("from", i),
                                                        format!("user_{i}"),
                                                    )
                                                    .size_px(12.0)
                                                    .show(ui);
                                                    Text::with_id(
                                                        ("msg", i),
                                                        "This is a longer message body that should wrap inside the Fill stack column without breaking words inside any single token.",
                                                    )
                                                    .size_px(13.0)
                                                    .wrapping()
                                                    .size((Sizing::FILL, Sizing::Hug))
                                                    .show(ui);
                                                });
                                        });
                                }
                                Panel::canvas()
                                    .size((Sizing::FILL, Sizing::Fixed(80.0)))
                                    .show(ui, |ui| {
                                        for i in 0..canvas_dots {
                                            Frame::with_id(("dot", i))
                                                .size((
                                                    Sizing::Fixed(16.0),
                                                    Sizing::Fixed(16.0),
                                                ))
                                                .position((
                                                    i as f32 * 22.0,
                                                    12.0 + (i % 3) as f32 * 18.0,
                                                ))
                                                .show(ui);
                                        }
                                    });
                            });
                    });
            });
    }

    fn time_with_mask(ui: &mut Ui, mask: u32, iters: u32) -> f64 {
        HASH_MASK.with(|m| m.set(mask));
        // warmup
        for _ in 0..50 {
            ui.tree.compute_hashes();
        }
        let t = Instant::now();
        for _ in 0..iters {
            ui.tree.compute_hashes();
        }
        let elapsed = t.elapsed().as_nanos() as f64;
        HASH_MASK.with(|m| m.set(M_ALL));
        elapsed / iters as f64
    }

    #[test]
    #[ignore]
    fn breakdown_compute_hashes() {
        let display = Display::from_physical(UVec2::new(1280, 800), 1.0);
        let mut ui = Ui::new();
        ui.begin_frame(display);
        build_scene(&mut ui, 16);
        let n = ui.tree.node_count();
        let iters = 5_000;

        let all = time_with_mask(&mut ui, M_ALL, iters);
        let none = time_with_mask(&mut ui, 0, iters);
        let only_layout = time_with_mask(&mut ui, M_LAYOUT, iters);
        let only_paint = time_with_mask(&mut ui, M_PAINT, iters);
        let only_extras = time_with_mask(&mut ui, M_EXTRAS, iters);
        let only_shapes = time_with_mask(&mut ui, M_SHAPES, iters);
        let only_grid = time_with_mask(&mut ui, M_GRID, iters);

        let no_layout = time_with_mask(&mut ui, M_ALL & !M_LAYOUT, iters);
        let no_paint = time_with_mask(&mut ui, M_ALL & !M_PAINT, iters);
        let no_extras = time_with_mask(&mut ui, M_ALL & !M_EXTRAS, iters);
        let no_shapes = time_with_mask(&mut ui, M_ALL & !M_SHAPES, iters);
        let no_grid = time_with_mask(&mut ui, M_ALL & !M_GRID, iters);

        eprintln!("\n=== compute_hashes breakdown — {n} nodes, {iters} iters ===");
        eprintln!(
            "ALL              : {all:>9.0} ns/call  ({:.1} ns/node)",
            all / n as f64
        );
        eprintln!(
            "NONE (loop+hash) : {none:>9.0} ns/call  ({:.1} ns/node)",
            none / n as f64
        );
        eprintln!("---- only ----");
        eprintln!(
            "LAYOUT only      : {only_layout:>9.0} ns  Δ {:>7.0}",
            only_layout - none
        );
        eprintln!(
            "PAINT  only      : {only_paint:>9.0} ns  Δ {:>7.0}",
            only_paint - none
        );
        eprintln!(
            "EXTRAS only      : {only_extras:>9.0} ns  Δ {:>7.0}",
            only_extras - none
        );
        eprintln!(
            "SHAPES only      : {only_shapes:>9.0} ns  Δ {:>7.0}",
            only_shapes - none
        );
        eprintln!(
            "GRID   only      : {only_grid:>9.0} ns  Δ {:>7.0}",
            only_grid - none
        );
        eprintln!("---- minus (cost = ALL - NO_X) ----");
        eprintln!("LAYOUT cost      : {:>9.0} ns", all - no_layout);
        eprintln!("PAINT  cost      : {:>9.0} ns", all - no_paint);
        eprintln!("EXTRAS cost      : {:>9.0} ns", all - no_extras);
        eprintln!("SHAPES cost      : {:>9.0} ns", all - no_shapes);
        eprintln!("GRID   cost      : {:>9.0} ns", all - no_grid);
    }
}
