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
use crate::element::{ElementExtras, LayoutCore, LayoutMode, PaintCore};
use crate::primitives::{Sizes, Sizing, Track};
use crate::shape::Shape;
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

/// Hash a value as its raw bytes in one `Hasher::write` call. Sound only
/// for `T` with no padding bytes — the caller is responsible for that.
/// Used here for homogeneous-primitive structs (`Spacing`, `Color`,
/// `Corners`, `Stroke`, `Size`, `Vec2`, `GridCell`), where uniform field
/// alignment rules out gaps.
///
/// Why this is faster: `FxHasher::write(&[u8])` consumes 8 bytes per
/// loop iteration and amortizes the rotate/multiply/xor cost across the
/// whole slice. Replacing N×`write_u32`/`write_u16` calls with one
/// `write` cuts the per-call overhead and lets the compiler keep more
/// state in registers.
#[inline]
fn pod<T>(h: &mut impl Hasher, v: &T) {
    let bytes =
        unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) };
    h.write(bytes);
}

/// `Sizing` is a tagged union with niche-uninit padding in its inactive
/// variant — `pod` would hash junk bytes. Encode as a deterministic
/// `tag:u8 + value:f32` instead. Inlined for the two `Sizes` axes.
#[inline]
fn hash_sizing(h: &mut impl Hasher, s: Sizing) {
    let (tag, v) = match s {
        Sizing::Fixed(v) => (0u8, v),
        Sizing::Hug => (1, 0.0),
        Sizing::Fill(w) => (2, w),
    };
    h.write_u8(tag);
    h.write_u32(v.to_bits());
}

#[inline]
fn hash_sizes(h: &mut impl Hasher, s: Sizes) {
    hash_sizing(h, s.w);
    hash_sizing(h, s.h);
}

/// Same shape as `hash_sizing`: tagged union, inactive payload bytes are
/// uninit, so explicit tag+payload encoding rather than `pod`. Packs the
/// 1-byte tag + optional 2-byte payload into a single 32-bit write
/// (high 16 bits zero for non-Grid variants).
#[inline]
fn hash_layout_mode(h: &mut impl Hasher, m: LayoutMode) {
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

fn hash_layout_core(h: &mut impl Hasher, l: &LayoutCore) {
    hash_layout_mode(h, l.mode);
    hash_sizes(h, l.size);
    pod(h, &l.padding);
    pod(h, &l.margin);
    // Pack Align (u8) + Visibility (u8 discriminant) into one u16 write.
    h.write_u16(((l.visibility as u8 as u16) << 8) | l.align.raw() as u16);
}

fn hash_paint_core(h: &mut impl Hasher, p: PaintCore) {
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

fn hash_node_extras(h: &mut impl Hasher, e: &ElementExtras) {
    // `transform` is intentionally omitted: it doesn't affect this
    // node's own paint (the encoder draws the node at its layout rect
    // *before* `PushTransform`; the transform composes into
    // descendants' screen rects via `Cascades`). A parent transform
    // change shows up as descendant screen-rect diffs in
    // `Damage::compute`, which is the right granularity.
    pod(h, &e.position);
    pod(h, &e.grid);
    pod(h, &e.min_size);
    pod(h, &e.max_size);
    h.write_u32(e.gap.to_bits());
    h.write_u32(e.line_gap.to_bits());
    h.write_u16(((e.child_align.raw() as u16) << 8) | e.justify as u8 as u16);
}

fn hash_shape(h: &mut impl Hasher, shape: &Shape) {
    match shape {
        Shape::RoundedRect {
            radius,
            fill,
            stroke,
        } => {
            h.write_u8(0);
            pod(h, radius);
            pod(h, fill);
            match stroke {
                None => h.write_u8(0),
                Some(s) => {
                    h.write_u8(1);
                    pod(h, s);
                }
            }
        }
        Shape::Line { a, b, width, color } => {
            h.write_u8(1);
            pod(h, a);
            pod(h, b);
            h.write_u32(width.to_bits());
            pod(h, color);
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
            pod(h, color);
            h.write_u32(font_size_px.to_bits());
            h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
        }
    }
}

fn hash_track(h: &mut impl Hasher, t: &Track) {
    hash_sizing(h, t.size);
    h.write_u32(t.min.to_bits());
    h.write_u32(t.max.to_bits());
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
    h.write_u32(def.row_gap.to_bits());
    h.write_u32(def.col_gap.to_bits());
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
