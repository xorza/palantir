use crate::gpu::frame::pass_recorder::{PassRecorder, PassState};
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::primitives::geometry::urect::URect;
use glam::Vec2;
use std::ops::Range;

/// A state call as the pass would take it, by handle value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Pipeline(u8),
    Viewport,
    Group(u32, u8),
    Vertex(u32, u8),
    Index(u8),
    Draw,
}

#[derive(Debug, Default)]
struct Recorder(Vec<Call>);

impl PassState for Recorder {
    type Pipeline = u8;
    type Layout = u8;
    type Group = u8;
    type Buffer = u8;

    fn set_pipeline(&mut self, pipeline: &u8) {
        self.0.push(Call::Pipeline(*pipeline));
    }

    fn set_bind_group(&mut self, index: u32, group: &u8) {
        self.0.push(Call::Group(index, *group));
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer: &u8) {
        self.0.push(Call::Vertex(slot, *buffer));
    }

    fn set_index_buffer(&mut self, buffer: &u8, _: wgpu::IndexFormat) {
        self.0.push(Call::Index(*buffer));
    }

    fn push_viewport(&mut self, _: ViewportPush) {
        self.0.push(Call::Viewport);
    }

    fn set_scissor_rect(&mut self, _: URect) {}

    fn set_stencil_reference(&mut self, _: u32) {}

    fn draw(&mut self, _: Range<u32>, _: Range<u32>) {
        self.0.push(Call::Draw);
    }

    fn draw_indexed(&mut self, _: Range<u32>, _: i32, _: Range<u32>) {
        self.0.push(Call::Draw);
    }
}

/// A quad's form switch sets the pipeline alone; a shadow's switches the layout, so pushes
/// the viewport, and adds its pair; a mask's buffer replaces the instance buffer; a group bound
/// for one draw is bound again after it; the same binding twice is one call.
#[test]
fn a_switch_sets_only_what_changed() {
    let [solid, gradient, shadow, mask, mesh] = [1u8, 2, 3, 4, 5];
    let [quad_layout, shadow_layout, mesh_layout] = [10u8, 11, 12];
    let [atlas, cutouts] = [20u8, 21];
    let [instances, corners, masks, mesh_vertices, mesh_indices] = [30u8, 31, 32, 33, 34];
    let mut rec = PassRecorder::new(
        Recorder::default(),
        ViewportPush {
            size: Vec2::splat(100.0),
            origin: Vec2::ZERO,
        },
    );
    let take = |rec: &mut PassRecorder<'_, Recorder>| std::mem::take(&mut rec.pass.0);

    for pipeline in [&solid, &gradient, &gradient] {
        rec.pipeline(pipeline, &quad_layout);
        rec.group(0, &atlas);
        rec.vertex(0, &instances);
    }
    assert_eq!(
        take(&mut rec),
        [
            Call::Pipeline(1),
            Call::Viewport,
            Call::Group(0, 20),
            Call::Vertex(0, 30),
            Call::Pipeline(2),
        ],
        "a form switch sets the pipeline alone, and the same run twice nothing"
    );

    rec.pipeline(&shadow, &shadow_layout);
    rec.group(0, &atlas);
    rec.vertex(0, &instances);
    rec.group(1, &cutouts);
    rec.vertex(1, &corners);
    assert_eq!(
        take(&mut rec),
        [
            Call::Pipeline(3),
            Call::Viewport,
            Call::Group(1, 21),
            Call::Vertex(1, 31)
        ]
    );

    rec.pipeline(&mask, &quad_layout);
    rec.group(0, &atlas);
    rec.vertex(0, &masks);
    assert_eq!(
        take(&mut rec),
        [Call::Pipeline(4), Call::Viewport, Call::Vertex(0, 32)]
    );

    let image = 22u8;
    rec.group_once(0, &image);
    rec.draw(0..4, 0..1);
    rec.group(0, &atlas);
    assert_eq!(
        take(&mut rec),
        [Call::Group(0, 22), Call::Draw, Call::Group(0, 20)],
        "a group bound for one draw is bound again after it"
    );

    rec.pipeline(&mesh, &mesh_layout);
    rec.vertex(0, &mesh_vertices);
    rec.index(&mesh_indices, wgpu::IndexFormat::Uint32);
    rec.index(&mesh_indices, wgpu::IndexFormat::Uint32);
    rec.index(&mesh_indices, wgpu::IndexFormat::Uint16);
    assert_eq!(
        take(&mut rec),
        [
            Call::Pipeline(5),
            Call::Viewport,
            Call::Vertex(0, 33),
            Call::Index(34),
            Call::Index(34)
        ],
        "an index buffer read as another format is bound again",
    );
}
