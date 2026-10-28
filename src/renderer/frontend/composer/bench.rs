#![expect(
    clippy::cast_sign_loss,
    reason = "the bench fixture's sizes are positive constants"
)]

//! The composer's replay of recorded paint into draw groups and batches, at a
//! typical and a heavy draw count: curves alone, then the higher-kind tiers
//! (meshes, images, text) where overlap decides whether a batch closes.

use crate::bench::backends::Backend;
use crate::bench::kind::{Kind, Scale};
use crate::common::span::Span;
use crate::display::Display;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::composer::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::rect_grid;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::paint::curve_basis::CurveBasis;
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
            cmds.draw_curve(
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

fn push_mesh(cmds: &mut PaintCapture, bbox: Rect) {
    cmds.draw_mesh(
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
    cmds.draw_text(
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
    cmds.draw_image(
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

/// `atomic/composer/cpu/curves/<count>` and
/// `atomic/composer/cpu/higher_kind_overlap/<case>/<count>`: one replay of recorded paint into
/// draw groups and batches, the groups and batches checked first. Per draw.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "composer", Scale::Sample);
    for curve_count in DRAW_COUNTS {
        group.throughput(Throughput::Elements(curve_count as u64));
        group.bench_function(Backend::Cpu.case(&format!("curves/{curve_count}")), |b| {
            let mut fixture = ComposeBench::curves(curve_count);
            assert_eq!(fixture.compose(), curve_count);
            b.iter(|| black_box(fixture.compose()));
        });
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
    group.finish();
    rect_grid::bench::atomic(c);
}
