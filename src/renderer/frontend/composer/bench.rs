#![expect(
    clippy::cast_sign_loss,
    reason = "the bench fixture's sizes are positive constants"
)]

//! The composer's replay of recorded paint into draw groups and batches, at a
//! typical and a heavy draw count: curves alone, the higher-kind tiers (meshes,
//! images, text) where overlap decides whether a batch closes, and the quad
//! lane: plain, clipped, under rounded clips, pruned, as backdrops, and a polyline; and the
//! rounded clip's arms, a quad, a text run and an image a cell.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::common::span::Span;
use crate::display::Display;
use crate::internals::compose_rig::ComposeRig;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::rect_grid;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::{
    ClosedEffect, EffectCaptureDraw,
};
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_stroke_payload::{
    DrawStrokePayload, PolylinePath, StrokePath,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::render_buffer::backdrop_row::BackdropCapture;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use criterion::{Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use strum::{IntoStaticStr, VariantArray};

/// A typical frame's draw count and a heavy one; per-draw cost is flat, so two
/// points show the slope.
const DRAW_COUNTS: [usize; 2] = [256, 4096];

#[derive(Debug)]
struct ComposeBench {
    cmds: PaintCapture,
    rig: ComposeRig,
}

impl ComposeBench {
    fn new(cmds: PaintCapture, physical: UVec2) -> Self {
        Self {
            cmds,
            rig: ComposeRig::new(Display::from_physical(physical, 1.0)),
        }
    }

    fn curves(curve_count: usize) -> Self {
        assert!(curve_count > 0);
        let mut cmds = PaintCapture::default();
        for _ in 0..curve_count {
            cmds.draw(
                DrawStrokePayload {
                    origin: Vec2::ZERO,
                    spin: None,
                    path: StrokePath::Curve(CurveBasis::Cubic(CubicBezier {
                        p0: Vec2::new(16.0, 64.0),
                        p1: Vec2::new(48.0, 64.0),
                        p2: Vec2::new(80.0, 64.0),
                        p3: Vec2::new(112.0, 64.0),
                    })),
                    fill: GpuFill {
                        color: RgbaF32::WHITE.to_f16(),
                        ..Default::default()
                    },
                    width: 2.0,
                    ..Default::default()
                },
                1.0,
            );
        }
        Self::new(cmds, UVec2::splat(128))
    }

    fn compose(&mut self) -> usize {
        self.rig.compose(&self.cmds);
        let out = &self.rig.out;
        out.meshes.len() + out.images.len() + out.curves.len()
    }
}

/// `IntoStaticStr` supplies the criterion id (the snake_case variant name), which can't drift from the variant.
#[derive(Clone, Copy, Debug, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
enum HigherKindCase {
    SameTierMesh,
    SameTierImage,
    MixedOverlap,
    MixedNonOverlap,
    /// One mesh per label, clear of it (the toolbar shape): every mesh pays the open batch's overlap query and none closes it, so the run is one text batch.
    TextBetweenMeshClear,
    /// Each mesh on its own label, so every query hits and every mesh closes the batch; the pair separates query cost from close cost.
    TextBetweenMeshOver,
}

impl HigherKindCase {
    /// Columns of the label grid the two text arms lay out.
    const TEXT_COLS: u32 = 16;

    /// The cell per label-plus-mesh pair; a 64 px text-grid tile holds about three labels, the occupancy the index targets.
    const TEXT_CELL: Vec2 = Vec2::new(64.0, 24.0);

    /// Where the label sits in its cell.
    const TEXT_LABEL: Rect = Rect::new(2.0, 4.0, 40.0, 16.0);

    /// Where the mesh sits: beside the label, or on it.
    const MESH_CLEAR: Rect = Rect::new(46.0, 4.0, 16.0, 16.0);
    const MESH_OVER: Rect = Rect::new(6.0, 6.0, 16.0, 12.0);

    fn fixture(self, draw_count: usize) -> ComposeBench {
        assert!(draw_count > 0);
        let mut cmds = PaintCapture::default();
        let overlap = Rect::new(16.0, 16.0, 32.0, 32.0);
        let disjoint = Rect::new(80.0, 80.0, 32.0, 32.0);
        match self {
            Self::SameTierMesh => {
                for _ in 0..draw_count {
                    push_mesh(&mut cmds, overlap);
                }
            }
            Self::SameTierImage => {
                for _ in 0..draw_count {
                    push_image(&mut cmds, overlap);
                }
            }
            Self::MixedOverlap | Self::MixedNonOverlap => {
                assert!(draw_count.is_multiple_of(2));
                let mesh_rect = if matches!(self, Self::MixedOverlap) {
                    overlap
                } else {
                    disjoint
                };
                for _ in 0..draw_count / 2 {
                    push_image(&mut cmds, overlap);
                    push_mesh(&mut cmds, mesh_rect);
                }
            }
            Self::TextBetweenMeshClear | Self::TextBetweenMeshOver => {
                let mesh = if matches!(self, Self::TextBetweenMeshOver) {
                    Self::MESH_OVER
                } else {
                    Self::MESH_CLEAR
                };
                for i in 0..draw_count {
                    let at = Vec2::new(
                        (i as u32 % Self::TEXT_COLS) as f32 * Self::TEXT_CELL.x,
                        (i as u32 / Self::TEXT_COLS) as f32 * Self::TEXT_CELL.y,
                    );
                    push_text(&mut cmds, cell_rect(at, Self::TEXT_LABEL));
                    push_mesh(&mut cmds, cell_rect(at, mesh));
                }
            }
        }
        ComposeBench::new(cmds, self.viewport(draw_count))
    }

    /// The display each arm composes against: 128 square for the stacked arms, the label grid (rounded up to whole rows) for the two text arms.
    const fn viewport(self, draw_count: usize) -> UVec2 {
        match self {
            Self::TextBetweenMeshClear | Self::TextBetweenMeshOver => {
                let rows = (draw_count as u32).div_ceil(Self::TEXT_COLS);
                UVec2::new(
                    Self::TEXT_COLS * Self::TEXT_CELL.x as u32,
                    rows * Self::TEXT_CELL.y as u32,
                )
            }
            Self::SameTierMesh
            | Self::SameTierImage
            | Self::MixedOverlap
            | Self::MixedNonOverlap => UVec2::splat(128),
        }
    }

    /// The runs the arm lists: one group of one tier's run for the same-tier arms; for the mixed
    /// arm that overlaps, a group a mesh over an image, `n / 2 + 1` groups of an image run, a
    /// mesh and an image run each, and a mesh run, `n` runs; one group of a mesh and an image run
    /// where nothing overlaps; one group of the text and a mesh run for the text arms, a text run
    /// a label where every mesh covers its label.
    const fn expected_runs(self, draw_count: usize) -> usize {
        match self {
            Self::MixedOverlap => draw_count,
            Self::SameTierMesh | Self::SameTierImage => 1,
            Self::MixedNonOverlap | Self::TextBetweenMeshClear => 2,
            Self::TextBetweenMeshOver => draw_count + 1,
        }
    }

    /// What the text arms separate: one batch for the grid when no mesh covers a label, one per label when all do.
    const fn expected_text_runs(self, draw_count: usize) -> usize {
        match self {
            Self::TextBetweenMeshClear => 1,
            Self::TextBetweenMeshOver => draw_count,
            Self::SameTierMesh
            | Self::SameTierImage
            | Self::MixedOverlap
            | Self::MixedNonOverlap => 0,
        }
    }
}

/// The quad lane's shapes: what it costs to order, clip, prune and list a run of quads.
#[derive(Clone, Copy, Debug, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
enum QuadCase {
    /// A grid of quads apart: one group, nothing to close.
    Plain,
    /// Each quad inside a rect clip of its own, which cuts nothing: one run.
    Clipped,
    /// Each quad inside a rounded clip of its own: a clip chain per quad, one run.
    RoundedClip,
    /// Every quad opaque over the one before: the group's prune keeps the last.
    Pruned,
    /// A grid of backdrops that hold their results: listed, never breaking the main pass.
    Backdrops,
    /// One polyline of that many points: its kept walk and its segment and joint instances.
    Polyline,
}

impl QuadCase {
    /// Columns of the grid the quads lay out in, a cell each.
    const COLS: u32 = 64;
    const CELL: f32 = 16.0;

    fn fixture(self, count: usize) -> ComposeBench {
        assert!(count > 0);
        let mut cmds = PaintCapture::default();
        let rows = (count as u32).div_ceil(Self::COLS);
        let viewport = UVec2::new(Self::COLS, rows) * Self::CELL as u32;
        let cell = |i: usize| {
            let at = Vec2::new(
                (i as u32 % Self::COLS) as f32,
                (i as u32 / Self::COLS) as f32,
            );
            Rect::new(
                at.x * Self::CELL + 2.0,
                at.y * Self::CELL + 2.0,
                Self::CELL - 4.0,
                Self::CELL - 4.0,
            )
        };
        let mut bench = ComposeBench::new(PaintCapture::default(), viewport);
        match self {
            Self::Plain => (0..count).for_each(|i| push_quad(&mut cmds, cell(i))),
            Self::Clipped | Self::RoundedClip => {
                for i in 0..count {
                    let corners = match self {
                        Self::RoundedClip => Corners::all(3.0),
                        _ => Corners::ZERO,
                    };
                    cmds.push_clip(PushClipPayload {
                        rect: cell(i),
                        corners,
                    });
                    push_quad(&mut cmds, cell(i));
                    cmds.pop_clip();
                }
            }
            Self::Pruned => {
                let rect = Rect::new(0.0, 0.0, Self::CELL * 4.0, Self::CELL * 4.0);
                (0..count).for_each(|_| push_quad(&mut cmds, rect));
            }
            Self::Backdrops => (0..count).for_each(|i| push_backdrop(&mut cmds, cell(i), i as u32)),
            Self::Polyline => {
                let store = &mut bench.rig.store;
                let p_start = store.polyline_points.len() as u32;
                let width = viewport.x as f32;
                store.polyline_points.extend((0..count).map(|i| {
                    let x = i as f32 / count as f32 * width;
                    Vec2::new(x, Self::CELL * (1.0 + (i % 2) as f32))
                }));
                cmds.draw(
                    DrawStrokePayload {
                        path: StrokePath::Polyline(PolylinePath {
                            bbox: Rect::new(0.0, Self::CELL, width, Self::CELL),
                            fit: FitMap::IDENTITY,
                            points: Span::new(p_start, count as u32),
                            colors: PolylineColorSpan::Single,
                            join: LineJoin::Miter,
                        }),
                        origin: Vec2::ZERO,
                        spin: None,
                        fill: GpuFill {
                            color: RgbaF32::WHITE.to_f16(),
                            ..Default::default()
                        },
                        width: 2.0,
                        cap: LineCap::Butt,
                    },
                    1.0,
                );
            }
        }
        bench.cmds = cmds;
        bench
    }

    /// What one replay leaves: quads, runs and listed backdrops for the quad arms, curve
    /// instances for the polyline, one segment per gap and one joint per interior point.
    const fn expected(self, count: usize) -> [usize; 3] {
        match self {
            Self::Plain | Self::Clipped | Self::RoundedClip => [count, 1, 0],
            Self::Pruned => [1, 1, 0],
            Self::Backdrops => [count, 1, count],
            Self::Polyline => [2 * count - 3, 1, 0],
        }
    }
}

/// The clip scene's arms (`rounded_clip::bench`) over recorded paint: per cell a quad, a text run
/// and an image under the cell's clips.
#[derive(Clone, Copy, Debug, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
enum ClipCase {
    /// A rect clip a cell, the control.
    Rect,
    /// A rounded clip a cell, its draws inset past every arc.
    Inside,
    /// A rounded clip a cell, its draws crossing every arc.
    Edge,
    /// Three rounded clips a cell, none holding the next, its draws crossing every arc.
    Nested,
}

impl ClipCase {
    const COLS: u32 = 64;
    const CELL: f32 = 32.0;
    const RADIUS: f32 = 6.0;
    /// How far a nested clip steps in, losing as much radius: its rect grown by a pixel crosses
    /// the outer arc.
    const NEST_STEP: f32 = 1.0;

    const fn depth(self) -> usize {
        match self {
            Self::Rect | Self::Inside | Self::Edge => 1,
            Self::Nested => 3,
        }
    }

    fn fixture(self, count: usize) -> ComposeBench {
        assert!(count > 0);
        let mut cmds = PaintCapture::default();
        let rows = (count as u32).div_ceil(Self::COLS);
        let viewport = UVec2::new(Self::COLS, rows) * Self::CELL as u32;
        for i in 0..count {
            let at = Vec2::new(
                (i as u32 % Self::COLS) as f32 * Self::CELL,
                (i as u32 / Self::COLS) as f32 * Self::CELL,
            );
            let mut clip = Rect::new(at.x, at.y, Self::CELL, Self::CELL);
            let mut radius = Self::RADIUS;
            for _ in 0..self.depth() {
                cmds.push_clip(PushClipPayload {
                    rect: clip,
                    corners: match self {
                        Self::Rect => Corners::ZERO,
                        Self::Inside | Self::Edge | Self::Nested => Corners::all(radius),
                    },
                });
                clip = clip.deflated(Self::NEST_STEP);
                radius -= Self::NEST_STEP;
            }
            let inset = match self {
                Self::Inside => Self::RADIUS,
                Self::Rect | Self::Edge | Self::Nested => 0.0,
            };
            let content = Rect::new(at.x, at.y, Self::CELL, Self::CELL).deflated(inset);
            push_quad(&mut cmds, content);
            push_text(&mut cmds, content);
            push_image(&mut cmds, content);
            for _ in 0..self.depth() {
                cmds.pop_clip();
            }
        }
        let mut bench = ComposeBench::new(PaintCapture::default(), viewport);
        bench.cmds = cmds;
        bench
    }
}

fn push_quad(cmds: &mut PaintCapture, rect: Rect) {
    cmds.draw(
        DrawClosedPayload::new(
            PlacedClosed::Box(PlacedBox {
                rect,
                corners: Corners::ZERO,
            }),
            ClosedFill::new(
                BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                LoweredBorder::NONE,
            ),
        ),
        1.0,
    );
}

/// A backdrop at `rect` that holds its result, so it lists without breaking the main pass.
fn push_backdrop(cmds: &mut PaintCapture, rect: Rect, i: u32) {
    let plan = BlurPlan::new(4.0);
    let footprint = URect::covering(rect.inflated(PlaneBackdrop::reach(plan, 0)));
    let material = LoweredMaterial::fixture(4.0);
    let read = EffectCaptureDraw {
        ticket: BackdropTicket {
            capture: BackdropCapture {
                key: BackdropKey {
                    owner: WidgetId::from_hash("bench.backdrop"),
                    shape: i,
                },
                footprint,
                plan,
                region: plan
                    .region(footprint, footprint.max())
                    .expect("a bench backdrop paints"),
            },
            captures: false,
        },
        reach: material.capture.reach,
    };
    cmds.draw(
        DrawClosedPayload::new(
            PlacedClosed::Box(PlacedBox {
                rect,
                corners: Corners::ZERO,
            }),
            ClosedEffect::material(
                material,
                read,
                BrushSource::Solid(RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16()),
                LoweredBorder::NONE,
            ),
        ),
        1.0,
    );
}

fn push_mesh(cmds: &mut PaintCapture, bbox: Rect) {
    cmds.draw(
        DrawMeshPayload {
            bbox,
            origin: Vec2::ZERO,
            fit: FitMap::IDENTITY,
            tint: RgbaF32::WHITE.to_f16(),
            vertices: Span::new(0, 3),
            idxs: Span::new(0, 3),
            spin: None,
        },
        1.0,
    );
}

/// `within` placed relative to a grid cell whose origin is `at`.
fn cell_rect(at: Vec2, within: Rect) -> Rect {
    Rect::new(
        at.x + within.min.x,
        at.y + within.min.y,
        within.size.w,
        within.size.h,
    )
}

fn push_text(cmds: &mut PaintCapture, rect: Rect) {
    cmds.draw(
        DrawTextPayload {
            rect,
            ink: Spacing::ZERO,
            color: RgbaF32::WHITE.to_f16(),
            text: ShapedTextRef {
                key: TextShapeKey::fixture(),
                span: Span::default(),
            },
            spin: None,
        },
        1.0,
    );
}

fn push_image(cmds: &mut PaintCapture, rect: Rect) {
    cmds.draw(
        DrawClosedPayload {
            geometry: PlacedClosed::Box(PlacedBox {
                rect,
                corners: Corners::ZERO,
            }),
            spin: None,
            paint: ClosedImage {
                rect,
                uv_min: Vec2::ZERO,
                uv_size: Vec2::ONE,
                tint: RgbaF32::WHITE.to_f16(),
                handle: TextureId(1),
                flags: ImageFlags::NONE,
                view: None,
            },
        },
        1.0,
    );
}

/// `atomic/composer/cpu/curves/alone/<count>`,
/// `atomic/composer/cpu/higher_kind_overlap/<case>/<count>`,
/// `atomic/composer/cpu/quads/<case>/<count>` and `atomic/composer/cpu/clips/<case>/<count>`: one replay of recorded paint into draw groups and
/// batches, what it leaves checked first. Per draw.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "composer", Scale::Sample);
    for curve_count in DRAW_COUNTS {
        group.throughput(Throughput::Elements(curve_count as u64));
        group.bench_function(
            Backend::Cpu.case(&format!("curves/alone/{curve_count}")),
            |b| {
                let mut fixture = ComposeBench::curves(curve_count);
                assert_eq!(fixture.compose(), curve_count);
                b.iter(|| black_box(fixture.compose()));
            },
        );
    }
    for &case in HigherKindCase::VARIANTS {
        let label: &'static str = case.into();
        for draw_count in DRAW_COUNTS {
            group.throughput(Throughput::Elements(draw_count as u64));
            let id = Backend::Cpu.case(&format!("higher_kind_overlap/{label}/{draw_count}"));
            group.bench_function(id, |b| {
                let mut fixture = case.fixture(draw_count);
                assert_eq!(fixture.compose(), draw_count);
                let runs = &fixture.rig.out.runs;
                assert_eq!(runs.len(), case.expected_runs(draw_count));
                assert_eq!(
                    runs.iter().filter(|run| run.lane == Lane::Text).count(),
                    case.expected_text_runs(draw_count),
                );
                b.iter(|| black_box(fixture.compose()));
            });
        }
    }
    for &case in QuadCase::VARIANTS {
        let label: &'static str = case.into();
        for count in DRAW_COUNTS {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(Backend::Cpu.case(&format!("quads/{label}/{count}")), |b| {
                let mut fixture = case.fixture(count);
                fixture.compose();
                let out = &fixture.rig.out;
                let drawn = match case {
                    QuadCase::Polyline => out.curves.len(),
                    QuadCase::Backdrops => out.effects.len(),
                    _ => out.quads.len(),
                };
                assert_eq!(
                    [drawn, out.runs.len(), out.backdrops.len()],
                    case.expected(count),
                    "{label}/{count}"
                );
                b.iter(|| black_box(fixture.compose()));
            });
        }
    }
    for &case in ClipCase::VARIANTS {
        let label: &'static str = case.into();
        for count in DRAW_COUNTS {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(Backend::Cpu.case(&format!("clips/{label}/{count}")), |b| {
                let mut fixture = case.fixture(count);
                fixture.compose();
                let out = &fixture.rig.out;
                assert_eq!(
                    [out.quads.len(), out.texts.len(), out.images.len()],
                    [count; 3],
                    "{label}/{count}"
                );
                b.iter(|| black_box(fixture.compose()));
            });
        }
    }
    group.finish();
    rect_grid::bench::atomic(c);
}
