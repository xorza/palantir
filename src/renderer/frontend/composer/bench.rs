#![expect(
    clippy::cast_sign_loss,
    reason = "the bench fixture's sizes are positive constants"
)]

//! The composer's replay of recorded paint into draw groups and batches, at a
//! typical and a heavy draw count: curves alone, the higher-kind tiers (meshes,
//! images, text) where overlap decides whether a batch closes, and the quad
//! lane: plain, clipped, under rounded clips, pruned, as panes, and a polyline.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::cascade::pane_row::PaneRow;
use crate::common::span::Span;
use crate::display::Display;
use crate::internals::compose_rig::ComposeRig;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::composer::rect_grid;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_pane_payload::DrawPanePayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, RectQuad};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::pane_captures::PaneTicket;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ColorMode;
use crate::shape::rect::RectKind;
use crate::shape::style::{LineCap, LineJoin};
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
                DrawCurvePayload {
                    bounds: StrokeBounds::Still(Rect::new(16.0, 63.0, 96.0, 2.0)),
                    origin: Vec2::ZERO,
                    basis: CurveBasis::Cubic(CubicBezier {
                        p0: Vec2::new(16.0, 64.0),
                        p1: Vec2::new(48.0, 64.0),
                        p2: Vec2::new(80.0, 64.0),
                        p3: Vec2::new(112.0, 64.0),
                    }),
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

    const fn expected_groups(self, draw_count: usize) -> usize {
        match self {
            Self::MixedOverlap => draw_count / 2 + 1,
            Self::SameTierMesh
            | Self::SameTierImage
            | Self::MixedNonOverlap
            | Self::TextBetweenMeshClear
            | Self::TextBetweenMeshOver => 1,
        }
    }

    /// What the text arms separate: one batch for the grid when no mesh covers a label, one per label when all do.
    const fn expected_text_batches(self, draw_count: usize) -> usize {
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
    /// Each quad inside a rect clip of its own: a group per quad.
    Clipped,
    /// Each quad inside a rounded clip of its own: a group and a mask chain per quad.
    RoundedClip,
    /// Every quad opaque over the one before: the group's prune keeps the last.
    Pruned,
    /// A grid of panes that hold their results: listed, never breaking the main pass.
    Panes,
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
            Self::Panes => (0..count).for_each(|i| push_pane(&mut cmds, cell(i), i as u32)),
            Self::Polyline => {
                let store = &mut bench.rig.store;
                let p_start = store.polyline_points.len() as u32;
                let width = viewport.x as f32;
                store.polyline_points.extend((0..count).map(|i| {
                    let x = i as f32 / count as f32 * width;
                    Vec2::new(x, Self::CELL * (1.0 + (i % 2) as f32))
                }));
                let c_start = store.polyline_colors.len() as u32;
                store.polyline_colors.push(RgbaF32::WHITE.to_f16());
                cmds.draw(
                    DrawPolylinePayload {
                        alpha: 1.0,
                        bounds: StrokeBounds::Still(Rect::new(0.0, Self::CELL, width, Self::CELL)),
                        origin: Vec2::ZERO,
                        width: 2.0,
                        points: Span::new(p_start, count as u32),
                        colors: Span::new(c_start, 1),
                        color_mode: ColorMode::Single,
                        cap: LineCap::Butt,
                        join: LineJoin::Miter,
                    },
                    1.0,
                );
            }
        }
        bench.cmds = cmds;
        bench
    }

    /// What one replay leaves: quads, groups and listed panes for the quad arms, curve instances
    /// for the polyline, one segment per gap and one joint per interior point.
    const fn expected(self, count: usize) -> [usize; 3] {
        match self {
            Self::Plain => [count, 1, 0],
            Self::Clipped | Self::RoundedClip => [count, count, 0],
            Self::Pruned => [1, 1, 0],
            Self::Panes => [count, 1, count],
            Self::Polyline => [2 * count - 3, 1, 0],
        }
    }
}

fn push_quad(cmds: &mut PaintCapture, rect: Rect) {
    cmds.draw(
        DrawQuadPayload::rect(RectQuad {
            kind: RectKind::Rounded,
            rect,
            corners: Corners::ZERO,
            fill: BrushSource::Solid(RgbaF32::WHITE.to_f16()),
            stroke: ShapeStroke::NONE,
        }),
        1.0,
    );
}

/// A pane at `rect` that holds its result, so it lists without breaking the main pass.
fn push_pane(cmds: &mut PaintCapture, rect: Rect, i: u32) {
    let plan = BlurPlan::new(4.0);
    let footprint = URect::covering(rect.inflated(PaneRow::reach(plan, 0)));
    cmds.draw(
        DrawPanePayload {
            kind: RectKind::Rounded,
            rect,
            corners: Corners::ZERO,
            spin: None,
            tint: RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16(),
            stroke: ShapeStroke::NONE,
            backdrop: LoweredBackdrop::from(BackdropFilter::blur(4.0)),
            opacity: 1.0,
            ticket: PaneTicket {
                key: PaneKey {
                    owner: WidgetId::from_hash("bench.pane"),
                    shape: i,
                },
                footprint,
                plan,
                region: plan
                    .region(footprint, footprint.max())
                    .expect("a bench pane paints"),
                captures: false,
            },
        },
        1.0,
    );
}

fn push_mesh(cmds: &mut PaintCapture, bbox: Rect) {
    cmds.draw(
        DrawMeshPayload {
            bbox,
            origin: Vec2::ZERO,
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
        ImageDraw {
            payload: DrawImagePayload {
                rect,
                uv_min: Vec2::ZERO,
                uv_size: Vec2::ONE,
                tint: RgbaF32::WHITE.to_f16(),
                handle: TextureId(1),
                flags: ImageFlags::NONE,
                spin: None,
            },
            view: None,
        },
        1.0,
    );
}

/// `atomic/composer/cpu/curves/alone/<count>`,
/// `atomic/composer/cpu/higher_kind_overlap/<case>/<count>` and
/// `atomic/composer/cpu/quads/<case>/<count>`: one replay of recorded paint into draw groups and
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
                assert_eq!(
                    fixture.rig.out.groups.len(),
                    case.expected_groups(draw_count)
                );
                assert_eq!(
                    fixture.rig.out.text_batches.len(),
                    case.expected_text_batches(draw_count),
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
                    _ => out.quads.len(),
                };
                assert_eq!(
                    [drawn, out.groups.len(), out.backdrops.len()],
                    case.expected(count),
                    "{label}/{count}"
                );
                b.iter(|| black_box(fixture.compose()));
            });
        }
    }
    group.finish();
    rect_grid::bench::atomic(c);
}
