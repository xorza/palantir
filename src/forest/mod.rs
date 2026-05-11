//! Per-layer arena collection. The forest holds one [`Tree`] per
//! `Layer` variant; `Ui::layer` switches the active tree, so popup
//! body recording dispatches into a different arena than `Main`
//! and never interleaves.

use crate::common::hash::Hasher as FxHasher;
use crate::forest::element::Element;
use crate::forest::seen_ids::SeenIds;
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::forest::widget_id::WidgetId;
use crate::layout::types::span::Span;
use crate::primitives::bezier::{
    eval_color_cubic, eval_color_quadratic, flatten_cubic, flatten_quadratic, lerp_color,
};
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::shape::{
    ColorMode, CubicBezierColors, LineCap, LineJoin, PolylineColors, QuadraticBezierColors, Shape,
    ShapeRecord,
};
use glam::Vec2;
use std::array;
use std::hash::Hasher as _;
use strum::EnumCount as _;

pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod rollups;
pub(crate) mod seen_ids;
pub(crate) mod tree;
pub(crate) mod visibility;
pub(crate) mod widget_id;

/// Recording-only state owned by [`Forest`]. The active layer selects
/// which `Tree` receives the next `open_node` / `add_shape`. The
/// anchor for the active scope's next root mint lives on the
/// destination tree's `pending_anchor` field — set by
/// [`Forest::push_layer`], consumed by the next `Tree::open_node`
/// that mints a `RootSlot`. Per-layer ancestor stacks live on each
/// `Tree` itself.
#[derive(Default)]
struct RecordingState {
    /// Active layer for the next `open_node`. `Main` between/outside
    /// `Ui::layer` scopes; switched by `push_scope` / `pop_scope`.
    current_layer: Layer,
    /// Save-stack: one entry per open `push_scope` — the outer layer
    /// is restored on `pop_scope`. Empty outside any layer scope.
    /// Anchors don't ride the stack because each `Tree` owns its own
    /// `pending_anchor` and same-layer nesting (which would clobber
    /// it) is forbidden by [`Forest::push_layer`]'s assert.
    layer_stack: Vec<Layer>,
}

impl RecordingState {
    fn reset(&mut self) {
        self.current_layer = Layer::Main;
        self.layer_stack.clear();
    }

    fn push_scope(&mut self, layer: Layer) {
        self.layer_stack.push(self.current_layer);
        self.current_layer = layer;
    }

    fn pop_scope(&mut self) {
        self.current_layer = self
            .layer_stack
            .pop()
            .expect("pop_scope called without a matching push_scope");
    }
}

/// One arena per [`Layer`]. Recording dispatches `open_node`,
/// `add_shape`, `close_node` to `trees[recording.current_layer as
/// usize]`. Pipeline passes iterate trees via
/// [`Forest::iter_paint_order`].
///
/// **Access convention**: prefer [`Forest::tree`] / [`Forest::tree_mut`]
/// for known-layer access; iterate `trees` directly only for
/// cross-layer aggregation that doesn't care about layer order
/// (e.g. summing record counts).
pub(crate) struct Forest {
    pub(crate) trees: [Tree; Layer::COUNT],
    recording: RecordingState,
    /// Forest-wide `WidgetId` tracker — collision detection across
    /// all layers, removed-widget diff, and frame rollover. Lives here
    /// (not on `Ui`) so the uniqueness invariant is enforced at the
    /// recording-arena layer instead of by orchestrator convention.
    pub(crate) ids: SeenIds,
}

impl Default for Forest {
    fn default() -> Self {
        Self {
            trees: array::from_fn(|_| Tree::default()),
            recording: RecordingState::default(),
            ids: SeenIds::default(),
        }
    }
}

impl Forest {
    pub(crate) fn begin_frame(&mut self) {
        self.recording.reset();
        self.ids.begin_frame();
        for t in &mut self.trees {
            t.begin_frame();
        }
    }

    /// Finalize every tree. `main_anchor` patches `Main`'s root slots
    /// (their anchor is the surface, only known after recording);
    /// other layers' anchors were stamped at `push_layer` time.
    pub(crate) fn end_frame(&mut self, main_anchor: Rect) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "end_frame called with active layer {:?} — Ui::layer body forgot to return",
            self.recording.current_layer,
        );
        for r in &mut self.trees[Layer::Main as usize].roots {
            r.anchor_rect = main_anchor;
        }
        for layer in Layer::PAINT_ORDER {
            self.trees[layer as usize].end_frame();
        }
    }

    pub(crate) fn open_node(&mut self, mut element: Element) -> NodeId {
        // Resolve the widget id at the recording boundary: builders
        // produce an unset id by default and chain `id_salt` /
        // `auto_id` to set it; explicit-id collisions hard-assert in
        // `SeenIds::record`, auto-id collisions get silently
        // disambiguated.
        assert!(
            element.id != WidgetId::default(),
            "widget recorded without a `WidgetId` — chain `.id_salt(key)`, \
             `.id(precomputed)`, or `.auto_id()` on the builder before `.show(ui)`. \
             `Foo::new()` no longer derives an id automatically.",
        );
        element.id = self.ids.record(element.id, element.auto_id);
        let layer = self.recording.current_layer;
        self.trees[layer as usize].open_node(element)
    }

    pub(crate) fn close_node(&mut self) {
        let layer = self.recording.current_layer;
        self.trees[layer as usize].close_node();
    }

    /// Convert a user-facing [`Shape`] into a [`ShapeRecord`] and push
    /// it onto the active tree. The Mesh arm copies vertex/index bytes
    /// into the active tree's mesh arenas and stamps spans into the
    /// record; the other three arms are field-for-field passthroughs.
    pub(crate) fn add_shape(&mut self, shape: Shape<'_>) {
        let tree = &mut self.trees[self.recording.current_layer as usize];
        let record = match shape {
            Shape::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => ShapeRecord::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            },
            Shape::Line {
                a,
                b,
                width,
                color,
                cap,
                join,
            } => lower_polyline(
                tree,
                &[a, b],
                PolylineColors::Single(color),
                width,
                cap,
                join,
            ),
            Shape::Polyline {
                points,
                colors,
                width,
                cap,
                join,
            } => lower_polyline(tree, points, colors, width, cap, join),
            Shape::CubicBezier {
                p0,
                p1,
                p2,
                p3,
                width,
                colors,
                cap,
                join,
                tolerance,
            } => lower_cubic_bezier(tree, p0, p1, p2, p3, width, colors, cap, join, tolerance),
            Shape::QuadraticBezier {
                p0,
                p1,
                p2,
                width,
                colors,
                cap,
                join,
                tolerance,
            } => lower_quadratic_bezier(tree, p0, p1, p2, width, colors, cap, join, tolerance),
            Shape::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => ShapeRecord::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            },
            Shape::Mesh {
                mesh,
                local_rect,
                tint,
            } => {
                let arena = &mut tree.shape_arenas.meshes;
                let v_start = arena.vertices.len() as u32;
                arena.vertices.extend_from_slice(&mesh.vertices);
                let i_start = arena.indices.len() as u32;
                arena.indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh.content_hash();
                ShapeRecord::Mesh {
                    local_rect,
                    tint,
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices: Span::new(i_start, mesh.indices.len() as u32),
                    content_hash,
                }
            }
        };
        tree.add_shape(record);
    }

    pub(crate) fn push_layer(&mut self, layer: Layer, anchor: Rect) {
        assert_eq!(
            self.recording.current_layer,
            Layer::Main,
            "Ui::layer must be called from the Main scope (current: {:?})",
            self.recording.current_layer,
        );
        self.trees[layer as usize].pending_anchor = anchor;
        self.recording.push_scope(layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let layer = self.recording.current_layer;
        assert!(
            self.trees[layer as usize].open_frames.is_empty(),
            "Ui::layer body left {} node(s) open in layer {:?}",
            self.trees[layer as usize].open_frames.len(),
            layer,
        );
        self.recording.pop_scope();
    }

    /// Borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree(&self, layer: Layer) -> &Tree {
        &self.trees[layer as usize]
    }

    /// Active recording layer. `Main` outside `Ui::layer` scopes; the
    /// scope's destination layer inside one. Read by widgets that need
    /// to know which arena their record stream is landing in (e.g.
    /// `Grid` / `Scroll` looking up the in-flight node id).
    #[inline]
    pub(crate) fn current_layer(&self) -> Layer {
        self.recording.current_layer
    }

    /// Active recording layer's `Tree::ancestor_disabled`. Read by
    /// `Ui::response_for` to OR inherited-disabled into the response
    /// state without waiting for next-frame cascade.
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.trees[self.recording.current_layer as usize].ancestor_disabled()
    }

    /// Mutably borrow the tree owned by `layer`.
    #[inline]
    pub(crate) fn tree_mut(&mut self, layer: Layer) -> &mut Tree {
        &mut self.trees[layer as usize]
    }

    /// Iterate trees in paint order (`Layer::PAINT_ORDER`), pairing
    /// each with its layer tag. Pipeline passes consume this to
    /// process layers bottom-up.
    pub(crate) fn iter_paint_order(&self) -> impl Iterator<Item = (Layer, &Tree)> {
        Layer::PAINT_ORDER
            .iter()
            .copied()
            .map(move |layer| (layer, &self.trees[layer as usize]))
    }
}

/// Lower a (points, colors, width) authoring shape into a
/// `ShapeRecord::Polyline`: validate `colors` length against
/// `points.len()`, copy both into the tree arenas, compute the
/// content hash. `Shape::Line` and `Shape::Polyline` both route
/// through this — one record path downstream.
fn lower_polyline(
    tree: &mut Tree,
    points: &[Vec2],
    colors: PolylineColors<'_>,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) -> ShapeRecord {
    let (mode, color_slice): (ColorMode, &[Color]) = match colors {
        PolylineColors::Single(ref c) => (ColorMode::Single, std::slice::from_ref(c)),
        PolylineColors::PerPoint(cs) => {
            assert_eq!(
                cs.len(),
                points.len(),
                "Shape::Polyline PerPoint colors len {} != points len {}",
                cs.len(),
                points.len(),
            );
            (ColorMode::PerPoint, cs)
        }
        PolylineColors::PerSegment(cs) => {
            assert_eq!(
                cs.len() + 1,
                points.len(),
                "Shape::Polyline PerSegment colors len {} != points len - 1 ({})",
                cs.len(),
                points.len().saturating_sub(1),
            );
            (ColorMode::PerSegment, cs)
        }
    };

    let arenas = &mut tree.shape_arenas;
    let p_start = arenas.polyline_points.len() as u32;
    arenas.polyline_points.extend_from_slice(points);
    let c_start = arenas.polyline_colors.len() as u32;
    arenas.polyline_colors.extend_from_slice(color_slice);

    let mut h = FxHasher::new();
    h.write(bytemuck::cast_slice(points));
    h.write(bytemuck::cast_slice(color_slice));
    h.write_u32(width.to_bits());
    h.write_u8(mode as u8);
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    let content_hash = h.finish();

    // Compute the owner-relative AABB once here so the encoder hot
    // path stays a straight `extend(map)`. Note: doesn't include
    // cap-extension; the composer inflates by the tessellator's
    // outer-fringe offset which already covers half-width
    // (sufficient for Butt and a tight upper bound for Square).
    let bbox = points_aabb(points);

    ShapeRecord::Polyline {
        width,
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, points.len() as u32),
        colors: Span::new(c_start, color_slice.len() as u32),
        bbox,
        content_hash,
    }
}

/// AABB of a non-empty point slice. Returns the zero rect on empty
/// input — `Shape::is_noop` filters `points.len() < 2` upstream so
/// the empty branch is defensive, not hot.
fn points_aabb(points: &[Vec2]) -> Rect {
    let Some((&first, rest)) = points.split_first() else {
        return Rect::ZERO;
    };
    let (mut lo, mut hi) = (first, first);
    for p in rest {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    Rect {
        min: lo,
        size: crate::primitives::size::Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    }
}

/// Lower [`Shape::CubicBezier`] into `ShapeRecord::Polyline` by
/// flattening into the tree's bezier scratch, copying points into
/// `polyline_points`, evaluating the color mode per-point into
/// `polyline_colors`, and stamping spans. `content_hash` covers the
/// *control points + colors + tolerance + width + cap + join* — the
/// flattened output is derived from these and shouldn't shift cache
/// identity by itself.
#[allow(clippy::too_many_arguments)]
fn lower_cubic_bezier(
    tree: &mut Tree,
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    width: f32,
    colors: CubicBezierColors,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    let arenas = &mut tree.shape_arenas;
    arenas.bezier_scratch.clear();
    flatten_cubic(p0, p1, p2, p3, tolerance, &mut arenas.bezier_scratch);

    let p_start = arenas.polyline_points.len() as u32;
    let n = arenas.bezier_scratch.len();
    let mut lo = arenas.bezier_scratch[0].p;
    let mut hi = lo;
    arenas.polyline_points.reserve(n);
    for fp in &arenas.bezier_scratch {
        arenas.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
    }

    let c_start = arenas.polyline_colors.len() as u32;
    let mode = match colors {
        CubicBezierColors::Solid(c) => {
            arenas.polyline_colors.push(c);
            ColorMode::Single
        }
        CubicBezierColors::Gradient2(a, b) => {
            arenas.polyline_colors.reserve(n);
            for fp in &arenas.bezier_scratch {
                arenas.polyline_colors.push(lerp_color(a, b, fp.t));
            }
            ColorMode::PerPoint
        }
        CubicBezierColors::Gradient3(a, b, c) => {
            arenas.polyline_colors.reserve(n);
            for fp in &arenas.bezier_scratch {
                arenas
                    .polyline_colors
                    .push(eval_color_quadratic(a, b, c, fp.t));
            }
            ColorMode::PerPoint
        }
        CubicBezierColors::Gradient4(a, b, c, d) => {
            arenas.polyline_colors.reserve(n);
            for fp in &arenas.bezier_scratch {
                arenas
                    .polyline_colors
                    .push(eval_color_cubic(a, b, c, d, fp.t));
            }
            ColorMode::PerPoint
        }
    };
    let c_len = arenas.polyline_colors.len() as u32 - c_start;

    let mut h = FxHasher::new();
    // Tag the variant so a polyline with the same numeric bytes
    // can't hash-collide with a bezier-derived record.
    h.write_u8(0xCB);
    h.write(bytemuck::bytes_of(&p0));
    h.write(bytemuck::bytes_of(&p1));
    h.write(bytemuck::bytes_of(&p2));
    h.write(bytemuck::bytes_of(&p3));
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    hash_cubic_colors(&mut h, &colors);
    let content_hash = h.finish();

    let bbox = Rect {
        min: lo,
        size: crate::primitives::size::Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    };

    ShapeRecord::Polyline {
        width,
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, c_len),
        bbox,
        content_hash,
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_quadratic_bezier(
    tree: &mut Tree,
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    width: f32,
    colors: QuadraticBezierColors,
    cap: LineCap,
    join: LineJoin,
    tolerance: f32,
) -> ShapeRecord {
    let arenas = &mut tree.shape_arenas;
    arenas.bezier_scratch.clear();
    flatten_quadratic(p0, p1, p2, tolerance, &mut arenas.bezier_scratch);

    let p_start = arenas.polyline_points.len() as u32;
    let n = arenas.bezier_scratch.len();
    let mut lo = arenas.bezier_scratch[0].p;
    let mut hi = lo;
    arenas.polyline_points.reserve(n);
    for fp in &arenas.bezier_scratch {
        arenas.polyline_points.push(fp.p);
        lo = lo.min(fp.p);
        hi = hi.max(fp.p);
    }

    let c_start = arenas.polyline_colors.len() as u32;
    let mode = match colors {
        QuadraticBezierColors::Solid(c) => {
            arenas.polyline_colors.push(c);
            ColorMode::Single
        }
        QuadraticBezierColors::Gradient2(a, b) => {
            arenas.polyline_colors.reserve(n);
            for fp in &arenas.bezier_scratch {
                arenas.polyline_colors.push(lerp_color(a, b, fp.t));
            }
            ColorMode::PerPoint
        }
        QuadraticBezierColors::Gradient3(a, b, c) => {
            arenas.polyline_colors.reserve(n);
            for fp in &arenas.bezier_scratch {
                arenas
                    .polyline_colors
                    .push(eval_color_quadratic(a, b, c, fp.t));
            }
            ColorMode::PerPoint
        }
    };
    let c_len = arenas.polyline_colors.len() as u32 - c_start;

    let mut h = FxHasher::new();
    h.write_u8(0xCB);
    h.write_u8(0x02); // quadratic discriminant
    h.write(bytemuck::bytes_of(&p0));
    h.write(bytemuck::bytes_of(&p1));
    h.write(bytemuck::bytes_of(&p2));
    h.write_u32(width.to_bits());
    h.write_u32(tolerance.to_bits());
    h.write_u8(cap as u8);
    h.write_u8(join as u8);
    hash_quadratic_colors(&mut h, &colors);
    let content_hash = h.finish();

    let bbox = Rect {
        min: lo,
        size: crate::primitives::size::Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    };

    ShapeRecord::Polyline {
        width,
        color_mode: mode,
        cap,
        join,
        points: Span::new(p_start, n as u32),
        colors: Span::new(c_start, c_len),
        bbox,
        content_hash,
    }
}

fn hash_cubic_colors(h: &mut FxHasher, colors: &CubicBezierColors) {
    match colors {
        CubicBezierColors::Solid(c) => {
            h.write_u8(0);
            h.write(bytemuck::bytes_of(c));
        }
        CubicBezierColors::Gradient2(a, b) => {
            h.write_u8(1);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
        }
        CubicBezierColors::Gradient3(a, b, c) => {
            h.write_u8(2);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
        }
        CubicBezierColors::Gradient4(a, b, c, d) => {
            h.write_u8(3);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
            h.write(bytemuck::bytes_of(d));
        }
    }
}

fn hash_quadratic_colors(h: &mut FxHasher, colors: &QuadraticBezierColors) {
    match colors {
        QuadraticBezierColors::Solid(c) => {
            h.write_u8(0);
            h.write(bytemuck::bytes_of(c));
        }
        QuadraticBezierColors::Gradient2(a, b) => {
            h.write_u8(1);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
        }
        QuadraticBezierColors::Gradient3(a, b, c) => {
            h.write_u8(2);
            h.write(bytemuck::bytes_of(a));
            h.write(bytemuck::bytes_of(b));
            h.write(bytemuck::bytes_of(c));
        }
    }
}
