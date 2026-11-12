//! [`PageGroup`]: one bind group over the pages of some of the pool's lists, as a draw that reads
//! them binds it, made again where a page's view changes.

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::texture_binding::{self, Filtering};

/// The layout of a [`PageGroup`] over `lists` lists: list `i`'s pages at bindings
/// `i · MAX_PAGES` up, page by page, no sampler, as the shaders read each texel by index.
pub(crate) fn layout(gpu: &Gpu, label: &str, lists: usize) -> wgpu::BindGroupLayout {
    let entries: Vec<_> = (0..lists * MAX_PAGES)
        .map(|binding| texture_binding::texture_entry(binding as u32, Filtering::Filtered))
        .collect();
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &entries,
        })
}

/// The pages of `N` lists, `formats`, bound together through a [`layout`] of `N` lists, which
/// the group holds, so every remake binds through the layout it was made with.
#[derive(Debug)]
pub(crate) struct PageGroup<const N: usize> {
    formats: [AtlasFormat; N],
    label: &'static str,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    /// The generation of each list's views `group` binds.
    bound: [u64; N],
}

impl<const N: usize> PageGroup<N> {
    pub(crate) fn new(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        label: &'static str,
        formats: [AtlasFormat; N],
        textures: &AtlasTextures,
    ) -> Self {
        Self {
            group: bind(gpu, layout, label, &formats, textures),
            bound: formats.map(|format| textures.generation(format)),
            formats,
            label,
            layout: layout.clone(),
        }
    }

    /// Make the group again where `textures` changed a page view since it was made: after the
    /// pool's commit and before any draw or bake of the frame reads it.
    pub(crate) fn follow(&mut self, gpu: &Gpu, textures: &AtlasTextures) {
        let bound = self.formats.map(|format| textures.generation(format));
        if bound != self.bound {
            self.group = bind(gpu, &self.layout, self.label, &self.formats, textures);
            self.bound = bound;
        }
    }

    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }
}

fn bind(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    formats: &[AtlasFormat],
    textures: &AtlasTextures,
) -> wgpu::BindGroup {
    let views: Vec<_> = formats
        .iter()
        .flat_map(|&format| textures.views(format))
        .collect();
    let entries: Vec<_> = views
        .iter()
        .enumerate()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}
