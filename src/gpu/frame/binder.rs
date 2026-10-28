//! What a main pass has bound, so each draw sets only the state that changed.

use crate::gpu::surface::viewport::ViewportPush;

/// The state calls a pass takes. wgpu records every one and drivers do not dedupe, so the
/// [`Binder`] elides repeats before they reach the pass. A trait so the elision is testable on a
/// recorder without a device.
pub(crate) trait PassState<'a> {
    type Pipeline;
    type Layout;
    type Group;
    type Buffer;

    fn set_pipeline(&mut self, pipeline: &'a Self::Pipeline);
    fn set_bind_group(&mut self, index: u32, group: &'a Self::Group);
    fn set_vertex_buffer(&mut self, slot: u32, buffer: &'a Self::Buffer);
    fn set_index_buffer(&mut self, buffer: &'a Self::Buffer, format: wgpu::IndexFormat);
    fn push_viewport(&mut self, viewport: ViewportPush);
}

impl<'a> PassState<'a> for wgpu::RenderPass<'a> {
    type Pipeline = wgpu::RenderPipeline;
    type Layout = wgpu::PipelineLayout;
    type Group = wgpu::BindGroup;
    type Buffer = wgpu::Buffer;

    fn set_pipeline(&mut self, pipeline: &'a wgpu::RenderPipeline) {
        Self::set_pipeline(self, pipeline);
    }

    fn set_bind_group(&mut self, index: u32, group: &'a wgpu::BindGroup) {
        Self::set_bind_group(self, index, group, &[]);
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer: &'a wgpu::Buffer) {
        Self::set_vertex_buffer(self, slot, buffer.slice(..));
    }

    fn set_index_buffer(&mut self, buffer: &'a wgpu::Buffer, format: wgpu::IndexFormat) {
        Self::set_index_buffer(self, buffer.slice(..), format);
    }

    fn push_viewport(&mut self, viewport: ViewportPush) {
        viewport.push_into(self);
    }
}

/// The binder of a main pass.
pub(crate) type MainBinder<'a> = Binder<'a, wgpu::RenderPass<'a>>;

/// Bind-group and vertex-buffer slots a draw of this backend uses: group 0, 1 (a shadow's cutout
/// tables) and 2 (a pane's backdrops), buffer 0 and 1 (a mesh's instances, a shadow's corners).
const SLOTS: usize = 3;

/// What a pass has bound, slot by slot, so a pipeline switch keeps the groups and buffers it
/// shares with the last one: a quad's form switch sets the pipeline alone.
///
/// The viewport rides in immediate data, which wgpu clears when the pipeline layout changes, so
/// it is pushed again exactly then (the rule `IMMEDIATES_BYTES` states).
#[derive(Debug)]
pub(crate) struct Binder<'a, S: PassState<'a>> {
    viewport: ViewportPush,
    pipeline: Option<&'a S::Pipeline>,
    layout: Option<&'a S::Layout>,
    groups: [Option<&'a S::Group>; SLOTS],
    buffers: [Option<&'a S::Buffer>; SLOTS],
    index: Option<IndexBinding<'a, S::Buffer>>,
}

#[derive(Debug)]
struct IndexBinding<'a, B> {
    buffer: &'a B,
    format: wgpu::IndexFormat,
}

impl<'a, S: PassState<'a>> Binder<'a, S> {
    pub(crate) const fn new(viewport: ViewportPush) -> Self {
        Self {
            viewport,
            pipeline: None,
            layout: None,
            groups: [None; SLOTS],
            buffers: [None; SLOTS],
            index: None,
        }
    }

    /// Bind `pipeline`, built over `layout`, unless it is bound.
    pub(crate) fn pipeline(
        &mut self,
        pass: &mut S,
        pipeline: &'a S::Pipeline,
        layout: &'a S::Layout,
    ) {
        if !same(self.pipeline, pipeline) {
            pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline);
        }
        if !same(self.layout, layout) {
            pass.push_viewport(self.viewport);
            self.layout = Some(layout);
        }
    }

    /// Bind `group` at `index`, unless it is bound there.
    pub(crate) fn group(&mut self, pass: &mut S, index: u32, group: &'a S::Group) {
        let slot = &mut self.groups[index as usize];
        if !same(*slot, group) {
            pass.set_bind_group(index, group);
            *slot = Some(group);
        }
    }

    /// Note that a draw set group `index` itself: an image's texture group lives no longer than
    /// the draw that reads it, so it cannot be tracked.
    pub(crate) const fn forget_group(&mut self, index: u32) {
        self.groups[index as usize] = None;
    }

    /// Bind the whole of `buffer` at vertex slot `slot`, unless it is bound there.
    pub(crate) fn vertex(&mut self, pass: &mut S, slot: u32, buffer: &'a S::Buffer) {
        let bound = &mut self.buffers[slot as usize];
        if !same(*bound, buffer) {
            pass.set_vertex_buffer(slot, buffer);
            *bound = Some(buffer);
        }
    }

    /// Bind the whole of `buffer` as indices of `format`, unless it is bound so.
    pub(crate) fn index(&mut self, pass: &mut S, buffer: &'a S::Buffer, format: wgpu::IndexFormat) {
        let bound = self
            .index
            .as_ref()
            .is_some_and(|index| std::ptr::eq(index.buffer, buffer) && index.format == format);
        if !bound {
            pass.set_index_buffer(buffer, format);
            self.index = Some(IndexBinding { buffer, format });
        }
    }
}

/// Whether `bound` is `item`, by address: a clone of a handle reads as another binding, which
/// costs a repeated call, never a missed one.
fn same<T>(bound: Option<&T>, item: &T) -> bool {
    bound.is_some_and(|bound| std::ptr::eq(bound, item))
}

#[cfg(test)]
mod tests {
    use crate::gpu::frame::binder::{Binder, PassState};
    use crate::gpu::surface::viewport::ViewportPush;
    use glam::Vec2;

    /// A state call as the pass would take it, by handle value.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Pipeline(u8),
        Viewport,
        Group(u32, u8),
        Vertex(u32, u8),
        Index(u8),
    }

    #[derive(Debug, Default)]
    struct Recorder(Vec<Call>);

    impl<'a> PassState<'a> for Recorder {
        type Pipeline = u8;
        type Layout = u8;
        type Group = u8;
        type Buffer = u8;

        fn set_pipeline(&mut self, pipeline: &'a u8) {
            self.0.push(Call::Pipeline(*pipeline));
        }

        fn set_bind_group(&mut self, index: u32, group: &'a u8) {
            self.0.push(Call::Group(index, *group));
        }

        fn set_vertex_buffer(&mut self, slot: u32, buffer: &'a u8) {
            self.0.push(Call::Vertex(slot, *buffer));
        }

        fn set_index_buffer(&mut self, buffer: &'a u8, _: wgpu::IndexFormat) {
            self.0.push(Call::Index(*buffer));
        }

        fn push_viewport(&mut self, _: ViewportPush) {
            self.0.push(Call::Viewport);
        }
    }

    /// A quad's form switch sets the pipeline alone; a shadow's switches the layout, so pushes
    /// the viewport, and adds its pair; a mask's buffer replaces the instance buffer; a group a
    /// draw set itself is bound again; the same binding twice is one call.
    #[test]
    fn a_switch_sets_only_what_changed() {
        let [solid, gradient, shadow, mask, mesh] = [1u8, 2, 3, 4, 5];
        let [quad_layout, shadow_layout, mesh_layout] = [10u8, 11, 12];
        let [atlas, cutouts] = [20u8, 21];
        let [instances, corners, masks, mesh_vertices, mesh_indices] = [30u8, 31, 32, 33, 34];
        let mut pass = Recorder::default();
        let mut binder = Binder::<Recorder>::new(ViewportPush {
            size: Vec2::splat(100.0),
        });
        let take = |pass: &mut Recorder| std::mem::take(&mut pass.0);

        for pipeline in [&solid, &gradient, &gradient] {
            binder.pipeline(&mut pass, pipeline, &quad_layout);
            binder.group(&mut pass, 0, &atlas);
            binder.vertex(&mut pass, 0, &instances);
        }
        assert_eq!(
            take(&mut pass),
            [
                Call::Pipeline(1),
                Call::Viewport,
                Call::Group(0, 20),
                Call::Vertex(0, 30),
                Call::Pipeline(2),
            ],
            "a form switch sets the pipeline alone, and the same run twice nothing"
        );

        binder.pipeline(&mut pass, &shadow, &shadow_layout);
        binder.group(&mut pass, 0, &atlas);
        binder.vertex(&mut pass, 0, &instances);
        binder.group(&mut pass, 1, &cutouts);
        binder.vertex(&mut pass, 1, &corners);
        assert_eq!(
            take(&mut pass),
            [
                Call::Pipeline(3),
                Call::Viewport,
                Call::Group(1, 21),
                Call::Vertex(1, 31)
            ]
        );

        binder.pipeline(&mut pass, &mask, &quad_layout);
        binder.group(&mut pass, 0, &atlas);
        binder.vertex(&mut pass, 0, &masks);
        assert_eq!(
            take(&mut pass),
            [Call::Pipeline(4), Call::Viewport, Call::Vertex(0, 32)]
        );

        binder.forget_group(0);
        binder.group(&mut pass, 0, &atlas);
        assert_eq!(
            take(&mut pass),
            [Call::Group(0, 20)],
            "after a draw's own group"
        );

        binder.pipeline(&mut pass, &mesh, &mesh_layout);
        binder.vertex(&mut pass, 0, &mesh_vertices);
        binder.index(&mut pass, &mesh_indices, wgpu::IndexFormat::Uint32);
        binder.index(&mut pass, &mesh_indices, wgpu::IndexFormat::Uint32);
        binder.index(&mut pass, &mesh_indices, wgpu::IndexFormat::Uint16);
        assert_eq!(
            take(&mut pass),
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
}
