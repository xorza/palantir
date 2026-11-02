//! A main pass and what it has bound, so each draw sets only the state that changed.

use crate::gpu::debug_marker;
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::primitives::geometry::urect::URect;
use std::ops::Range;

/// The calls a pass takes. wgpu records every one and drivers do not dedupe, so the
/// [`PassRecorder`] elides repeated state before it reaches the pass. A trait so the elision is
/// testable on a recorder without a device.
pub(crate) trait PassState {
    type Pipeline;
    type Layout;
    type Group;
    type Buffer;

    fn set_pipeline(&mut self, pipeline: &Self::Pipeline);
    fn set_bind_group(&mut self, index: u32, group: &Self::Group);
    fn set_vertex_buffer(&mut self, slot: u32, buffer: &Self::Buffer);
    fn set_index_buffer(&mut self, buffer: &Self::Buffer, format: wgpu::IndexFormat);
    fn push_viewport(&mut self, viewport: ViewportPush);
    fn set_scissor_rect(&mut self, rect: URect);
    fn set_stencil_reference(&mut self, reference: u32);
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>);
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>);
}

impl PassState for wgpu::RenderPass<'_> {
    type Pipeline = wgpu::RenderPipeline;
    type Layout = wgpu::PipelineLayout;
    type Group = wgpu::BindGroup;
    type Buffer = wgpu::Buffer;

    fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        Self::set_pipeline(self, pipeline);
    }

    fn set_bind_group(&mut self, index: u32, group: &wgpu::BindGroup) {
        Self::set_bind_group(self, index, group, &[]);
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer: &wgpu::Buffer) {
        Self::set_vertex_buffer(self, slot, buffer.slice(..));
    }

    fn set_index_buffer(&mut self, buffer: &wgpu::Buffer, format: wgpu::IndexFormat) {
        Self::set_index_buffer(self, buffer.slice(..), format);
    }

    fn push_viewport(&mut self, viewport: ViewportPush) {
        viewport.push_into(self);
    }

    fn set_scissor_rect(&mut self, rect: URect) {
        Self::set_scissor_rect(self, rect.min.x, rect.min.y, rect.size.x, rect.size.y);
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        Self::set_stencil_reference(self, reference);
    }

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        Self::draw(self, vertices, instances);
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        Self::draw_indexed(self, indices, base_vertex, instances);
    }
}

/// Bind-group and vertex-buffer slots a draw of this backend uses: group 0, 1 (a shadow's cutout
/// tables) and 2 (a pane's backdrops), buffer 0 and 1 (a mesh's instances, a shadow's corners).
const SLOTS: usize = 3;

/// A main pass and what it has bound, slot by slot, so a pipeline switch keeps the groups and
/// buffers it shares with the last one: a quad's form switch sets the pipeline alone. It owns the
/// pass, so every draw records through the elision and none can set state behind it. Refused:
/// the binding state and the pass as two arguments, where a draw that set a group on the pass
/// itself had to remember to tell the state.
///
/// The viewport rides in immediate data, which wgpu clears when the pipeline layout changes, so
/// it is pushed again exactly then (the rule `IMMEDIATES_BYTES` states).
#[derive(Debug)]
pub(crate) struct PassRecorder<'a, S: PassState = wgpu::RenderPass<'a>> {
    pass: S,
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

impl<'a, S: PassState> PassRecorder<'a, S> {
    /// Record into `pass`, which has bound nothing, its draws placed by `viewport`.
    pub(crate) const fn new(pass: S, viewport: ViewportPush) -> Self {
        Self {
            pass,
            viewport,
            pipeline: None,
            layout: None,
            groups: [None; SLOTS],
            buffers: [None; SLOTS],
            index: None,
        }
    }

    /// Bind `pipeline`, built over `layout`, unless it is bound.
    pub(crate) fn pipeline(&mut self, pipeline: &'a S::Pipeline, layout: &'a S::Layout) {
        if !same(self.pipeline, pipeline) {
            self.pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline);
        }
        if !same(self.layout, layout) {
            self.pass.push_viewport(self.viewport);
            self.layout = Some(layout);
        }
    }

    /// Bind `group` at `index`, unless it is bound there.
    pub(crate) fn group(&mut self, index: u32, group: &'a S::Group) {
        let slot = &mut self.groups[index as usize];
        if !same(*slot, group) {
            self.pass.set_bind_group(index, group);
            *slot = Some(group);
        }
    }

    /// Bind `group` at `index` for the next draw alone, as an image's texture group, which lives
    /// no longer than the draw that reads it, so the slot reads as unbound after.
    pub(crate) fn group_once(&mut self, index: u32, group: &S::Group) {
        self.pass.set_bind_group(index, group);
        self.groups[index as usize] = None;
    }

    /// Bind the whole of `buffer` at vertex slot `slot`, unless it is bound there.
    pub(crate) fn vertex(&mut self, slot: u32, buffer: &'a S::Buffer) {
        let bound = &mut self.buffers[slot as usize];
        if !same(*bound, buffer) {
            self.pass.set_vertex_buffer(slot, buffer);
            *bound = Some(buffer);
        }
    }

    /// Bind the whole of `buffer` as indices of `format`, unless it is bound so.
    pub(crate) fn index(&mut self, buffer: &'a S::Buffer, format: wgpu::IndexFormat) {
        let bound = self
            .index
            .as_ref()
            .is_some_and(|index| std::ptr::eq(index.buffer, buffer) && index.format == format);
        if !bound {
            self.pass.set_index_buffer(buffer, format);
            self.index = Some(IndexBinding { buffer, format });
        }
    }

    /// Clip the draws that follow to `rect`, in the pass's target pixels.
    pub(crate) fn scissor(&mut self, rect: URect) {
        self.pass.set_scissor_rect(rect);
    }

    pub(crate) fn stencil_reference(&mut self, reference: u32) {
        self.pass.set_stencil_reference(reference);
    }

    pub(crate) fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draw(vertices, instances);
    }

    pub(crate) fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) {
        self.pass.draw_indexed(indices, base_vertex, instances);
    }

    /// The pass, for the calls that bind nothing a draw reads: timestamps and debug groups.
    pub(crate) const fn untracked(&mut self) -> &mut S {
        &mut self.pass
    }
}

/// A render pass's debug groups, the capture labels a frame's steps carry.
impl PassRecorder<'_> {
    /// Open a debug group named `label` on the pass, where the `gpu-debug-markers` feature is on.
    #[inline]
    pub(crate) fn push_marker(&mut self, label: &str) {
        if debug_marker::ENABLED {
            self.pass.push_debug_group(label);
        }
    }

    /// Close the debug group [`Self::push_marker`] opened.
    #[inline]
    pub(crate) fn pop_marker(&mut self) {
        if debug_marker::ENABLED {
            self.pass.pop_debug_group();
        }
    }
}

/// Whether `bound` is `item`, by address: a clone of a handle reads as another binding, which
/// costs a repeated call, never a missed one.
fn same<T>(bound: Option<&T>, item: &T) -> bool {
    bound.is_some_and(|bound| std::ptr::eq(bound, item))
}

#[cfg(test)]
mod tests;
