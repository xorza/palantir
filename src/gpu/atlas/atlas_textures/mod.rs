//! [`AtlasTextures`]: the pool's GPU half, the textures behind its pages and scratch slots, made
//! to follow the CPU half at each commit.

pub(crate) mod move_draw;

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
use crate::gpu::atlas::atlas_textures::move_draw::{MoveDraw, MoveRect, MoveRun};
use crate::gpu::atlas::page_list::{MAX_PAGES, Move, MoveBudget, PageList};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::texel;
use crate::gpu::texel::TargetSpec;
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
/// a blur or a move renders into it, and a grow or a move copies out of it.
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
    /// `mover` moves each of the list's moves from the page it left to its new one, in the order
    /// they were made, as a later one may land where an item moved and then evicted lay; then a
    /// page closed drops its texture, and a scratch slot makes or drops its texture as its size
    /// comes and goes. A move's source is the texture of the page opening it left, whether its
    /// slot still holds that opening or closed it, or took another, this commit.
    fn commit(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        list: &PageList,
        mover: &mut Mover,
    ) {
        let moves = list.moves();
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
        let source = |m: &Move| {
            let at = usize::from(m.from.page);
            [&self.pages[at], &previous[at]]
                .into_iter()
                .flatten()
                .find(|texture| texture.epoch == m.from_epoch)
                .expect("a move's source page was committed")
        };
        let dest = |m: &Move| {
            self.pages[usize::from(m.to.page)]
                .as_ref()
                .expect("a move lands on an open page")
        };
        match mover {
            Mover::Copy => {
                for m in moves {
                    debug_assert_eq!(m.from.size, m.to.size, "a move keeps its item's size");
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            origin: texel::origin(m.from.origin.as_uvec2()),
                            ..source(m).bound.texture.as_image_copy()
                        },
                        wgpu::TexelCopyTextureInfo {
                            origin: texel::origin(m.to.origin.as_uvec2()),
                            ..dest(m).bound.texture.as_image_copy()
                        },
                        texel::extent(m.from.size.as_uvec2()),
                    );
                }
            }
            Mover::Draw { draw, rects } => {
                // One page drains at a time and no drain is planned over waiting moves, so a
                // commit's moves leave one page: a run a destination page, in the moves' order.
                debug_assert!(
                    moves.iter().all(|m| m.from_epoch == moves[0].from_epoch),
                    "a commit's moves leave one page"
                );
                for page in 0..MAX_PAGES {
                    let mut landing = moves
                        .iter()
                        .filter(|m| usize::from(m.to.page) == page)
                        .peekable();
                    let Some(&first) = landing.peek() else {
                        continue;
                    };
                    let to = dest(first);
                    rects.clear();
                    rects.extend(landing.map(|&m| MoveRect::of(m, to.bound.size().x)));
                    draw.record(
                        gpu,
                        encoder,
                        MoveRun {
                            format: self.format,
                            source: &source(first).bound.view,
                            dest: &to.bound.view,
                            rects,
                        },
                    );
                }
            }
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

/// How a device moves a drain's items between pages: by copy commands where a pass moves its
/// whole target, as a draw onto a page would load and store all of it, and by one draw a
/// destination page elsewhere, WebRender's two routes (`src/gpu/AGENTS.md`, Atlases).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveRoute {
    Copy,
    Draw,
}

impl MoveRoute {
    /// The route of a device whose passes move `traffic`.
    pub(crate) const fn of(traffic: PassTraffic) -> Self {
        match traffic {
            PassTraffic::WholeTarget => Self::Copy,
            PassTraffic::Drawn => Self::Draw,
        }
    }

    /// What a display frame's moves may take on it.
    pub(crate) const fn budget(self) -> MoveBudget {
        match self {
            Self::Copy => MoveBudget::COPIES,
            Self::Draw => MoveBudget::DRAWN,
        }
    }
}

/// What a commit moves items with: copy commands, or [`MoveDraw`] and the scratch one run's
/// rectangles gather in, kept across commits.
#[derive(Debug)]
enum Mover {
    Copy,
    Draw {
        draw: MoveDraw,
        /// One run's rectangles.
        rects: Vec<MoveRect>,
    },
}

/// The textures behind every list of an [`AtlasPool`], made again by its constructor on a new
/// device and filled again by the tenants, as every device object is
/// (`src/gpu/AGENTS.md`, Device loss).
#[derive(Debug)]
pub(crate) struct AtlasTextures {
    lists: PerFormat<ListTextures>,
    mover: Mover,
}

impl AtlasTextures {
    /// The textures of a device, which moves items by its [`MoveRoute`].
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self::with_route(gpu, MoveRoute::of(gpu.pass_traffic()))
    }

    /// The textures of a device, moving items by `route`.
    fn with_route(gpu: &Gpu, route: MoveRoute) -> Self {
        Self {
            lists: PerFormat::from_fn(|format| ListTextures::new(gpu, format)),
            mover: match route {
                MoveRoute::Copy => Mover::Copy,
                MoveRoute::Draw => Mover::Draw {
                    draw: MoveDraw::new(gpu),
                    rects: Vec::new(),
                },
            },
        }
    }

    /// What a display frame's moves may take on this device, which the pool's decay takes.
    pub(crate) const fn move_budget(&self) -> MoveBudget {
        let route = match self.mover {
            Mover::Copy => MoveRoute::Copy,
            Mover::Draw { .. } => MoveRoute::Draw,
        };
        route.budget()
    }

    /// Make every texture follow `pool`, a grow's and the drains' copies recorded on `encoder`,
    /// the moves taken from `pool`: ahead of every upload, bake and draw that writes or reads them.
    pub(crate) fn commit(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        pool: &mut AtlasPool,
    ) {
        if let Mover::Draw { draw, .. } = &mut self.mover {
            draw.begin();
        }
        for (format, textures) in self.lists.iter_mut() {
            textures.commit(gpu, encoder, pool.list(format), &mut self.mover);
        }
        pool.forget_moves();
    }

    /// The views of `format`'s pages by page index, a closed page's the 1 × 1 stand-in.
    pub(crate) fn views(&self, format: AtlasFormat) -> [&wgpu::TextureView; MAX_PAGES] {
        let list = &self.lists[format];
        std::array::from_fn(|idx| {
            list.pages[idx]
                .as_ref()
                .map_or(&list.absent.view, |page| &page.bound.view)
        })
    }

    /// Moves whenever a view of `format`'s pages or scratch textures changes: a consumer that binds
    /// them makes its group again when it differs from the one it made the group at.
    pub(crate) fn generation(&self, format: AtlasFormat) -> u64 {
        self.lists[format].generation
    }

    /// The view of `format`'s page `idx`, `None` where none is open.
    pub(crate) fn page_view(&self, format: AtlasFormat, idx: usize) -> Option<&wgpu::TextureView> {
        self.lists[format].pages[idx]
            .as_ref()
            .map(|page| &page.bound.view)
    }

    /// The texture of `format`'s page `idx`, `None` where none is open.
    pub(crate) fn page_texture(&self, format: AtlasFormat, idx: usize) -> Option<&wgpu::Texture> {
        self.lists[format].pages[idx]
            .as_ref()
            .map(|page| &page.bound.texture)
    }

    /// The 1 × 1 view of `format` a binding with no texture of its own binds.
    pub(crate) fn absent_view(&self, format: AtlasFormat) -> &wgpu::TextureView {
        &self.lists[format].absent.view
    }

    /// The view of `format`'s scratch slot `idx`, `None` where it holds no texture.
    pub(crate) fn scratch_view(&self, format: AtlasFormat, idx: u32) -> Option<&wgpu::TextureView> {
        self.lists[format]
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
            self.lists[format].bytes()
        }

        /// `format`'s textures, named `name`, against the texels its pages' items and its held
        /// scratch textures fill in `pool`.
        pub(crate) fn memory(
            &self,
            pool: &AtlasPool,
            format: AtlasFormat,
            name: &'static str,
        ) -> AtlasUse {
            let textures = &self.lists[format];
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
