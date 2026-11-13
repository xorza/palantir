//! [`AtlasGroup`]: the one bind group over the pool's mask and color pages, which every draw that
//! reads a page binds at [`ATLAS_GROUP`], a raster's and a shadow's alike, and [`AtlasSlots`], the
//! open slots their pipelines are built for. `primitives/wgsl/atlas_pages.wgsl` declares the pages
//! in this order and takes the group's index by substitution.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::pipeline_recipe;
use crate::gpu::resource::texture_binding::{self, Filtering, TextureBinding};

/// The group the atlas binds at, past the frame group.
pub(crate) const ATLAS_GROUP: u32 = 1;

/// The lists the atlas group binds, list `i`'s pages at bindings `i · MAX_PAGES` up: the masks,
/// which glyphs, icons and the shadows' cutout tables share, then the color glyphs and icons.
pub(crate) const ATLAS_LISTS: [AtlasFormat; 2] = [AtlasFormat::R8, AtlasFormat::Rgba8];

/// The override each list's open slots pin, as `primitives/wgsl/atlas_pages.wgsl` names it.
const SLOT_OVERRIDES: [&str; 2] = ["PALANTIR_MASK_SLOTS", "PALANTIR_COLOR_SLOTS"];

/// The open slots of the lists the atlas group binds, after the pool's commit: what the pipelines
/// that read the pages are built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasSlots {
    /// The `R8` list's: glyph and icon masks, and the shadows' cutout tables.
    pub(crate) mask: PageSlots,
    /// The `Rgba8` list's: color glyphs and icons.
    pub(crate) color: PageSlots,
}

impl AtlasSlots {
    /// Slot 0 of each list.
    pub(crate) const FIRST: Self = Self {
        mask: PageSlots::FIRST,
        color: PageSlots::FIRST,
    };

    /// The slots `pool`'s lists hold open now.
    pub(crate) fn of(pool: &AtlasPool) -> Self {
        let [mask, color] = ATLAS_LISTS.map(|format| pool.list(format).slots());
        Self { mask, color }
    }

    /// The overrides a pipeline over every list's pages is built with.
    pub(crate) fn constants(self) -> [(&'static str, f64); 2] {
        let [mask, color] = SLOT_OVERRIDES;
        [
            (mask, f64::from(self.mask.bits())),
            (color, f64::from(self.color.bits())),
        ]
    }

    /// The override a pipeline over the mask pages alone is built with: a shadow's, which reads
    /// its cutout tables there.
    pub(crate) fn mask_constant(self) -> (&'static str, f64) {
        self.constants()[0]
    }
}

/// The group over [`ATLAS_LISTS`]' pages and its layout, made again where a page's view changes.
#[derive(Debug)]
pub(crate) struct AtlasGroup {
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    /// The generation of each list's views `group` binds.
    bound: [u64; 2],
}

impl AtlasGroup {
    /// The group over `textures`' pages. Its layout's entries are loaded, no sampler, as the
    /// shaders read each texel by index and filter by hand.
    pub(crate) fn new(gpu: &Gpu, textures: &AtlasTextures) -> Self {
        let entries: Vec<_> = (0..ATLAS_LISTS.len() * MAX_PAGES)
            .map(|binding| texture_binding::texture_entry(binding as u32, Filtering::Loaded))
            .collect();
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.atlas.bgl"),
                entries: &entries,
            });
        Self {
            group: bind(gpu, &layout, textures),
            bound: generations(textures),
            layout,
        }
    }

    /// Make the group again where `textures` changed a page view since it was made: after the
    /// pool's commit and before any draw or bake of the frame reads it.
    pub(crate) fn follow(&mut self, gpu: &Gpu, textures: &AtlasTextures) {
        let bound = generations(textures);
        if bound != self.bound {
            self.group = bind(gpu, &self.layout, textures);
            self.bound = bound;
        }
    }

    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    /// The one pipeline layout the main pass's fills, shadows, rasters, meshes and curves build
    /// over: the frame group of `textures`, then this group, so a switch among them keeps the
    /// layout, and with it the immediates the viewport rides in. A family that reads no page binds
    /// this group all the same, which the recorder elides while it stays bound
    /// (`src/gpu/AGENTS.md`, Shapes).
    pub(crate) fn pipeline_layout(
        &self,
        gpu: &Gpu,
        textures: &TextureBinding,
    ) -> wgpu::PipelineLayout {
        pipeline_recipe::pipeline_layout(
            gpu,
            "palantir.main.pl",
            &[Some(textures.frame_layout()), Some(&self.layout)],
        )
    }
}

fn generations(textures: &AtlasTextures) -> [u64; 2] {
    ATLAS_LISTS.map(|format| textures.generation(format))
}

fn bind(gpu: &Gpu, layout: &wgpu::BindGroupLayout, textures: &AtlasTextures) -> wgpu::BindGroup {
    let views = ATLAS_LISTS
        .iter()
        .flat_map(|&format| textures.views(format));
    let entries: Vec<_> = (0..)
        .zip(views)
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("palantir.atlas.bg"),
        layout,
        entries: &entries,
    })
}
