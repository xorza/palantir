use crate::bench::Run;
use crate::display::Display;
use crate::primitives::color::RgbaF32;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::render_buffer::RenderBuffer;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::CurveBasis;
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::{UVec2, Vec2};
use std::hint::black_box;
use std::time::Duration;
use strum::{IntoStaticStr, VariantArray};

#[derive(Debug)]
struct ComposeBench {
    cmds: PaintCapture,
    store: RecordStore,
    composer: Composer,
    out: RenderBuffer,
    display: Display,
}

impl ComposeBench {
    fn new(cmds: PaintCapture, physical: UVec2) -> Self {
        Self {
            cmds,
            store: RecordStore::default(),
            composer: Composer::new(8192),
            out: RenderBuffer::new(),
            display: Display::from_physical(physical, 1.0),
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
                    basis: CurveBasis::Cubic {
                        p0: Vec2::new(16.0, 64.0),
                        p1: Vec2::new(48.0, 64.0),
                        p2: Vec2::new(80.0, 64.0),
                        p3: Vec2::new(112.0, 64.0),
                    },
                    fill: GpuFill {
                        color: RgbaF32::WHITE.into(),
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
        self.composer
            .begin(self.display, Duration::ZERO, &self.store, &mut self.out)
            .replay_from(&self.cmds);
        self.out.meshes.len() + self.out.images.len() + self.out.curves.len()
    }
}

/// `IntoStaticStr` supplies the criterion id: the variant name in
/// snake_case is the label every arm wanted anyway, and a derived one
/// cannot drift from the variant it names.
#[derive(Clone, Copy, Debug, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
enum HigherKindCase {
    SameTierMesh,
    SameTierImage,
    MixedOverlap,
    MixedNonOverlap,
    /// One mesh per label, clear of it: the toolbar shape. Every mesh
    /// pays the open batch's overlap query and none of them closes it,
    /// so the whole run is one text batch.
    TextBetweenMeshClear,
    /// The same layout with each mesh moved onto its own label, so every
    /// query hits and every mesh closes the batch. The pair is what
    /// separates the query's cost from the close's.
    TextBetweenMeshOver,
}

impl HigherKindCase {
    /// Columns of the label grid the two text arms lay out.
    const TEXT_COLS: u32 = 16;

    /// The cell that grid gives each label-plus-mesh pair. A label and
    /// its mesh fit side by side in one, and a 64 px text-grid tile then
    /// holds about three labels — the occupancy the index is built for.
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

    /// The display each arm composes against: 128 square for the arms
    /// that stack every draw at one spot, and the whole label grid —
    /// rounded up to whole rows — for the two that lay one out.
    fn viewport(self, draw_count: usize) -> UVec2 {
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

    fn expected_groups(self, draw_count: usize) -> usize {
        match self {
            Self::MixedOverlap => draw_count / 2 + 1,
            Self::SameTierMesh
            | Self::SameTierImage
            | Self::MixedNonOverlap
            | Self::TextBetweenMeshClear
            | Self::TextBetweenMeshOver => 1,
        }
    }

    /// The number the two text arms exist to separate: one batch for the
    /// whole grid when no mesh covers a label, one per label when they
    /// all do.
    fn expected_text_batches(self, draw_count: usize) -> usize {
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
            tint: RgbaF32::WHITE.into(),
            v_start: 0,
            v_len: 3,
            i_start: 0,
            i_len: 3,
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
            color: RgbaF32::WHITE.into(),
            text: ShapedTextRef {
                key: TextShapeKey::fixture(),
                span: Span::default(),
            },
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
                tint: RgbaF32::WHITE.into(),
                handle: TextureId(1),
                flags: 0,
            },
            paint: None,
        },
        1.0,
    );
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "curves");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for curve_count in [64, 256, 1024, 4096] {
        let mut fixture = ComposeBench::curves(curve_count);
        assert_eq!(fixture.compose(), curve_count);
        group.throughput(Throughput::Elements(curve_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(curve_count),
            &curve_count,
            |b, _| b.iter(|| black_box(fixture.compose())),
        );
    }
    group.finish();

    let mut group = run.subgroup(c, "higher_kind_overlap");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for &case in HigherKindCase::VARIANTS {
        let label: &'static str = case.into();
        for draw_count in [64, 256, 1024, 4096] {
            let mut fixture = case.fixture(draw_count);
            assert_eq!(fixture.compose(), draw_count);
            assert_eq!(fixture.out.groups.len(), case.expected_groups(draw_count));
            assert_eq!(
                fixture.out.text_batches.len(),
                case.expected_text_batches(draw_count),
            );
            group.throughput(Throughput::Elements(draw_count as u64));
            group.bench_with_input(BenchmarkId::new(label, draw_count), &draw_count, |b, _| {
                b.iter(|| black_box(fixture.compose()))
            });
        }
    }
    group.finish();
}
