//! [`AtlasTextures`]: the pool's GPU half, the textures behind its pages and scratch slots, made
//! to follow the CPU half at each commit.

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::page_list::{MAX_PAGES, Move, PageList};
use crate::gpu::atlas::{AtlasPool, ListCommit};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::owned_target::TargetSpec;
use crate::gpu::texel;
use glam::UVec2;

/// The texture format of `format`'s pages: color pages are linear, the raster shader decoding
/// their sRGB, so a move copies their bytes exactly on every backend
/// (`src/gpu/AGENTS.md`, Atlases).
pub(crate) const fn texture_format(format: AtlasFormat) -> wgpu::TextureFormat {
    match format {
        AtlasFormat::R8 => wgpu::TextureFormat::R8Unorm,
        AtlasFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        AtlasFormat::Rgba16F => wgpu::TextureFormat::Rgba16Float,
    }
}

/// What every page and scratch texture takes: a draw reads it, a raster writes into it, a bake,
/// a blur or a move renders into it, and a grow copies out of it. Render use on every format, as
/// M2 picks the move mechanism per device and may pick a draw.
const USAGE: wgpu::TextureUsages = wgpu::TextureUsages::TEXTURE_BINDING
    .union(wgpu::TextureUsages::RENDER_ATTACHMENT)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// A texture and the view that binds it.
#[derive(Debug)]
struct Bound {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Bound {
    fn new(gpu: &Gpu, label: &str, format: AtlasFormat, size: UVec2) -> Self {
        let texture = texel::texture_2d(
            gpu,
            TargetSpec {
                label,
                size,
                format: texture_format(format),
                usage: USAGE,
            },
        );
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            texture,
        }
    }

    fn size(&self) -> UVec2 {
        texel::size(&self.texture)
    }
}

/// A page's texture and the opening of its slot it was made for ([`Page::epoch`]).
///
/// [`Page::epoch`]: crate::gpu::atlas::page_list::Page::epoch
#[derive(Debug)]
struct PageTexture {
    bound: Bound,
    epoch: u32,
}

/// One list's textures.
#[derive(Debug)]
struct ListTextures {
    format: AtlasFormat,
    pages: [Option<PageTexture>; MAX_PAGES],
    /// By scratch slot.
    scratch: Vec<Option<Bound>>,
    /// A 1 × 1 texture of the format, which a view slot of a closed page or an empty scratch
    /// binding binds.
    absent: Bound,
    /// Moves whenever a page's or a scratch texture's view changes, so a consumer makes its group
    /// again.
    generation: u64,
}

impl ListTextures {
    fn new(gpu: &Gpu, format: AtlasFormat) -> Self {
        Self {
            format,
            pages: [const { None }; MAX_PAGES],
            scratch: Vec::new(),
            absent: Bound::new(gpu, "palantir.atlas.absent", format, UVec2::ONE),
            generation: 0,
        }
    }

    /// Make the textures follow `list`, recording on `encoder`: a page opened makes its texture, a
    /// page grown in its slot makes a larger one and copies the old texels into its top-left; then
    /// each of `moves` copies its item from the page it left to its new one; then a page closed
    /// drops its texture, and a scratch slot makes or drops its texture as its size comes and
    /// goes. A move's source is the texture of the page opening it left, whether its slot still
    /// holds that opening or closed it, or took another, this commit.
    fn commit(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        list: &PageList,
        moves: impl Iterator<Item = Move>,
    ) {
        let mut previous = std::mem::replace(&mut self.pages, [const { None }; MAX_PAGES]);
        for (idx, held) in previous.iter_mut().enumerate() {
            let Some(page) = list.page(idx) else {
                if held.is_some() {
                    self.generation += 1;
                }
                continue;
            };
            let edge = page.edge();
            let same = |held: &PageTexture| held.epoch == page.epoch();
            if held
                .as_ref()
                .is_some_and(|held| same(held) && held.bound.size().x == edge)
            {
                self.pages[idx] = held.take();
                continue;
            }
            let bound = Bound::new(gpu, "palantir.atlas.page", self.format, UVec2::splat(edge));
            if let Some(held) = held.as_ref().filter(|held| same(held)) {
                let old = held.bound.size();
                debug_assert!(old.x < edge, "a page grows in its slot, never shrinks");
                encoder.copy_texture_to_texture(
                    held.bound.texture.as_image_copy(),
                    bound.texture.as_image_copy(),
                    texel::extent(old),
                );
            }
            self.pages[idx] = Some(PageTexture {
                bound,
                epoch: page.epoch(),
            });
            self.generation += 1;
        }
        for Move {
            from,
            from_epoch,
            to,
        } in moves
        {
            let at = usize::from(from.page);
            let source = [&self.pages[at], &previous[at]]
                .into_iter()
                .flatten()
                .find(|texture| texture.epoch == from_epoch)
                .expect("a move's source page was committed");
            let dest = self.pages[usize::from(to.page)]
                .as_ref()
                .expect("a move lands on an open page");
            debug_assert_eq!(from.size, to.size, "a move keeps its item's size");
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    origin: texel::origin(from.origin.as_uvec2()),
                    ..source.bound.texture.as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    origin: texel::origin(to.origin.as_uvec2()),
                    ..dest.bound.texture.as_image_copy()
                },
                texel::extent(from.size.as_uvec2()),
            );
        }
        drop(previous);
        let slots = list.scratch().slots();
        self.scratch.resize_with(slots.len(), || None);
        for (texture, size) in self.scratch.iter_mut().zip(slots) {
            match (size, texture.as_ref()) {
                (None, None) => continue,
                (Some(size), Some(held)) if held.size() == size => continue,
                (None, Some(_)) => *texture = None,
                (Some(size), _) => {
                    *texture = Some(Bound::new(gpu, "palantir.atlas.scratch", self.format, size));
                }
            }
            self.generation += 1;
        }
    }
}

/// The textures behind every list of an [`AtlasPool`], made again by its constructor on a new
/// device and filled again by the tenants, as every device object is
/// (`src/gpu/AGENTS.md`, Device loss).
#[derive(Debug)]
pub(crate) struct AtlasTextures {
    lists: [ListTextures; 3],
}

impl AtlasTextures {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            lists: AtlasFormat::ALL.map(|format| ListTextures::new(gpu, format)),
        }
    }

    /// Make every texture follow `pool`, a grow's and the drains' copies recorded on `encoder`,
    /// the moves taken from `pool`: ahead of every upload, bake and draw that writes or reads them.
    pub(crate) fn commit(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        pool: &mut AtlasPool,
    ) {
        for (textures, format) in self.lists.iter_mut().zip(AtlasFormat::ALL) {
            let ListCommit { list, moves } = pool.commit_list(format);
            textures.commit(gpu, encoder, list, moves);
        }
    }

    /// The views of `format`'s pages by page index, a closed page's the 1 × 1 stand-in.
    pub(crate) fn views(&self, format: AtlasFormat) -> [&wgpu::TextureView; MAX_PAGES] {
        let list = &self.lists[format as usize];
        std::array::from_fn(|idx| {
            list.pages[idx]
                .as_ref()
                .map_or(&list.absent.view, |page| &page.bound.view)
        })
    }

    /// Moves whenever a view of `format`'s pages or scratch textures changes: a consumer that binds
    /// them makes its group again when it differs from the one it made the group at.
    pub(crate) const fn generation(&self, format: AtlasFormat) -> u64 {
        self.lists[format as usize].generation
    }

    /// The texture of `format`'s page `idx`, `None` where none is open.
    pub(crate) fn page_texture(&self, format: AtlasFormat, idx: usize) -> Option<&wgpu::Texture> {
        self.lists[format as usize].pages[idx]
            .as_ref()
            .map(|page| &page.bound.texture)
    }

    /// The 1 × 1 view of `format` a binding with no texture of its own binds.
    pub(crate) const fn absent_view(&self, format: AtlasFormat) -> &wgpu::TextureView {
        &self.lists[format as usize].absent.view
    }

    /// The view of `format`'s scratch slot `idx`, `None` where it holds no texture.
    pub(crate) fn scratch_view(&self, format: AtlasFormat, idx: u32) -> Option<&wgpu::TextureView> {
        self.lists[format as usize]
            .scratch
            .get(idx as usize)?
            .as_ref()
            .map(|bound| &bound.view)
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::atlas::atlas_format::AtlasFormat;
    use crate::gpu::atlas::atlas_textures::{AtlasTextures, ListTextures};
    use crate::gpu::atlas::page_list::Page;
    use crate::gpu::atlas_memory::AtlasUse;
    use crate::gpu::texel;

    impl ListTextures {
        fn bytes(&self) -> u64 {
            let pages = self.pages.iter().flatten().map(|page| &page.bound.texture);
            let scratch = self.scratch.iter().flatten().map(|bound| &bound.texture);
            pages
                .chain(scratch)
                .map(|texture| {
                    let size = texel::size(texture);
                    u64::from(size.x) * u64::from(size.y)
                })
                .sum::<u64>()
                * u64::from(self.format.texel_bytes())
        }
    }

    impl AtlasTextures {
        /// The bytes of `format`'s page and scratch textures, which the CPU half counts as
        /// [`PageList::bytes`](crate::gpu::atlas::page_list::PageList::bytes) once committed.
        pub(crate) fn bytes(&self, format: AtlasFormat) -> u64 {
            self.lists[format as usize].bytes()
        }

        /// `format`'s textures, named `name`, against the texels its pages' items and its held
        /// scratch textures fill in `pool`.
        pub(crate) fn memory(
            &self,
            pool: &AtlasPool,
            format: AtlasFormat,
            name: &'static str,
        ) -> AtlasUse {
            let textures = &self.lists[format as usize];
            let count =
                textures.pages.iter().flatten().count() + textures.scratch.iter().flatten().count();
            let list = pool.list(format);
            let live: u64 = (0..textures.pages.len())
                .filter_map(|idx| list.page(idx))
                .map(Page::live_texels)
                .sum::<u64>()
                + list.scratch().held_texels();
            AtlasUse {
                name,
                textures: u32::try_from(count).expect("a list holds few textures"),
                bytes: self.bytes(format),
                live_bytes: Some(live * u64::from(format.texel_bytes())),
            }
        }
    }
}

#[cfg(test)]
mod tests;
