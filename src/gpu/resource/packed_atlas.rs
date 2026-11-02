//! A square texture of packed rectangles: its shelf packer, its edge grown by doubling up to a
//! byte budget and past it up to the device's edge, and the copy of its texels into the grown
//! texture.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::shelf_packer::ShelfPacker;
use crate::gpu::surface::render_target;
use glam::{U16Vec2, UVec2};

/// What every texture of one atlas shares through its doublings.
#[derive(Debug)]
pub(crate) struct AtlasSpec {
    /// GPU debug name, built once so a grow reuses it.
    pub(crate) label: String,
    pub(crate) format: wgpu::TextureFormat,
    /// How the atlas's owner uses its texture, beside the copies every atlas takes.
    pub(crate) usage: wgpu::TextureUsages,
    /// The device's largest texture edge, which no grow passes.
    pub(crate) ceiling: u32,
    /// The bytes [`PackedAtlas::grow`] grows the texture to; only
    /// [`PackedAtlas::grow_past_budget`] grows it past them.
    pub(crate) max_bytes: u64,
}

impl AtlasSpec {
    /// The largest edge whose texels fit the budget, inside the ceiling and the packer's 16-bit
    /// edge.
    fn budget_edge(&self) -> u32 {
        let texel = self
            .format
            .block_copy_size(None)
            .expect("an atlas format is a color format");
        let by_bytes = (self.max_bytes / u64::from(texel)).isqrt();
        let edge = u32::try_from(by_bytes)
            .unwrap_or(u32::MAX)
            .min(self.max_edge());
        debug_assert!(
            edge > 0,
            "a budget of {} bytes holds no texel",
            self.max_bytes
        );
        edge
    }

    /// The ceiling, inside the packer's 16-bit edge.
    const fn max_edge(&self) -> u32 {
        if self.ceiling < ShelfPacker::MAX_SIZE {
            self.ceiling
        } else {
            ShelfPacker::MAX_SIZE
        }
    }

    fn create(&self, gpu: &Gpu, edge: u32) -> wgpu::Texture {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&self.label),
            size: render_target::extent(UVec2::splat(edge)),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: self.usage | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }
}

/// A square texture whose rectangles a [`ShelfPacker`] places.
///
/// A grow doubles the packer's edge at once and the texture at [`Self::commit`], so a frame that
/// grows several times makes one texture at the final edge and copies its texels once, where a
/// texture a doubling would make each intermediate one and never sample it.
///
/// The atlas grows on its own up to its byte budget, which its owner evicts within. Past it only
/// an owner that would otherwise drop what the frame shows grows it, up to the device's edge, and
/// the texture never shrinks back.
#[derive(Debug)]
pub(crate) struct PackedAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The edge the packer places rectangles on; the texture's until a grow, which
    /// [`Self::commit`] makes the texture follow.
    edge: u32,
    /// [`AtlasSpec::budget_edge`].
    budget_edge: u32,
    packer: ShelfPacker,
    spec: AtlasSpec,
}

impl PackedAtlas {
    /// An empty atlas of `spec` at an edge of `initial` texels, held to the budget.
    pub(crate) fn new(gpu: &Gpu, spec: AtlasSpec, initial: u32) -> Self {
        let budget_edge = spec.budget_edge();
        let edge = initial.min(budget_edge);
        let texture = spec.create(gpu, edge);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            edge,
            budget_edge,
            packer: ShelfPacker::new(edge_u16(edge)),
            spec,
        }
    }

    /// The edge rectangles are placed on, in texels.
    pub(crate) const fn edge(&self) -> u32 {
        self.edge
    }

    /// Whether a rectangle of `size` fits the edge as it stands.
    pub(crate) const fn fits_now(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.edge)
    }

    /// Whether a rectangle of `size` could be placed without growing past the budget: freeing
    /// rectangles cannot widen the texture.
    pub(crate) const fn fits_budget(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.budget_edge)
    }

    /// Whether a rectangle of `size` could ever be placed, the atlas grown past its budget to the
    /// device's edge.
    pub(crate) const fn fits_ceiling(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.spec.max_edge())
    }

    pub(crate) const fn packer(&self) -> &ShelfPacker {
        &self.packer
    }

    /// The top-left of a free rectangle of `size`, now taken; `None` when the edge as it stands
    /// has no room.
    pub(crate) fn allocate(&mut self, size: U16Vec2) -> Option<U16Vec2> {
        self.packer.allocate(size)
    }

    /// Return the rectangle of `width` at `origin`.
    pub(crate) fn deallocate(&mut self, origin: U16Vec2, width: u16) {
        self.packer.deallocate(origin, width);
    }

    /// Double the edge, held to the budget, keeping every placement; `false` at the budget.
    pub(crate) fn grow(&mut self) -> bool {
        self.grow_to(self.budget_edge)
    }

    /// [`Self::grow`] past the budget, held to the device's edge; `false` at that edge.
    pub(crate) fn grow_past_budget(&mut self) -> bool {
        self.grow_to(self.spec.max_edge())
    }

    fn grow_to(&mut self, limit: u32) -> bool {
        if self.edge >= limit {
            return false;
        }
        self.edge = (self.edge * 2).min(limit);
        self.packer.grow(edge_u16(self.edge));
        true
    }

    /// Make the texture follow the edge after a grow: one texture at the edge, the old one's
    /// texels copied into its top-left by `encoder`, ahead of whatever the frame writes there.
    /// `true` when the texture moved, so whatever binds its view binds the new one.
    pub(crate) fn commit(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) -> bool {
        let old = self.texture.width();
        if old == self.edge {
            return false;
        }
        let texture = self.spec.create(gpu, self.edge);
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            texture.as_image_copy(),
            render_target::extent(UVec2::splat(old)),
        );
        self.view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture = texture;
        true
    }

    /// Whether a grow still owes its [`Self::commit`].
    pub(crate) fn has_pending_grow(&self) -> bool {
        self.texture.width() != self.edge
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

/// An edge as the packer counts it, which [`AtlasSpec::max_edge`] holds to 16 bits.
fn edge_u16(edge: u32) -> u16 {
    u16::try_from(edge).expect("an atlas edge fits the packer's 16-bit edge")
}

/// Whether a rectangle of `size` fits inside a square of `edge` texels.
const fn fits_edge(size: U16Vec2, edge: u32) -> bool {
    size.x as u32 <= edge && size.y as u32 <= edge
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::packed_atlas::{AtlasSpec, PackedAtlas};
    use crate::gpu::test_gpu::headless_test_gpu;
    use glam::U16Vec2;
    use std::iter;

    fn spec(format: wgpu::TextureFormat, ceiling: u32, max_bytes: u64) -> AtlasSpec {
        AtlasSpec {
            label: "test.packed_atlas".to_owned(),
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            ceiling,
            max_bytes,
        }
    }

    /// The budget edge is the largest whose texels fit the bytes, not the device's edge: 16 MiB,
    /// `2^24`, is 4096² of 1-byte texels and 2048² of 4-byte ones, whatever edge the device
    /// allows past them. A device edge below it binds, and so does the packer's 16-bit edge.
    /// Quadrupling the bytes doubles the edge; a budget between two squares takes the smaller,
    /// `isqrt(5) = 2`.
    #[test]
    fn the_budget_edge_is_the_largest_square_the_bytes_hold() {
        use wgpu::TextureFormat::{R8Unorm, Rgba8UnormSrgb, Rgba16Float};
        const BUDGET: u64 = 16 << 20;
        for ceiling in [8192, 16384, 32768] {
            assert_eq!(spec(R8Unorm, ceiling, BUDGET).budget_edge(), 4096);
            assert_eq!(spec(Rgba8UnormSrgb, ceiling, BUDGET).budget_edge(), 2048);
        }
        assert_eq!(spec(R8Unorm, 1024, BUDGET).budget_edge(), 1024);
        assert_eq!(spec(Rgba8UnormSrgb, 512, BUDGET).budget_edge(), 512);
        assert_eq!(spec(R8Unorm, 1 << 17, u64::MAX).budget_edge(), 65535);
        assert_eq!(spec(Rgba8UnormSrgb, 16384, BUDGET * 4).budget_edge(), 4096);
        assert_eq!(spec(R8Unorm, 16384, BUDGET / 4).budget_edge(), 2048);
        assert_eq!(spec(Rgba16Float, 16384, 5 * 8).budget_edge(), 2);
    }

    /// Two grows in one frame move the packer's edge twice and make one texture, at the final
    /// edge, when the frame commits: 16 to the budget's 32, then past it to the 64 ceiling. The
    /// old texels land in its top-left, texel for texel, and the rest is the new texture's zeros;
    /// a commit with no grow moves nothing.
    #[test]
    fn two_grows_make_one_texture_that_keeps_the_texels() {
        let gpu = headless_test_gpu();
        let mut atlas = PackedAtlas::new(&gpu, spec(wgpu::TextureFormat::R8Unorm, 64, 32 * 32), 16);
        let texels: Vec<u8> = (0..=255u8).collect();
        gpu.queue.write_texture(
            atlas.texture().as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            atlas.texture().size(),
        );
        assert_eq!(atlas.allocate(U16Vec2::new(16, 16)), Some(U16Vec2::ZERO));
        assert_eq!(
            atlas.allocate(U16Vec2::new(4, 4)),
            None,
            "the 16² edge is full"
        );
        assert!(atlas.fits_budget(U16Vec2::splat(32)) && !atlas.fits_budget(U16Vec2::splat(33)));
        assert!(atlas.fits_ceiling(U16Vec2::splat(64)) && !atlas.fits_ceiling(U16Vec2::splat(65)));
        assert!(atlas.grow());
        assert!(!atlas.grow(), "at the budget");
        assert_eq!(atlas.edge(), 32);
        assert!(atlas.grow_past_budget());
        assert!(!atlas.grow_past_budget(), "at the ceiling");
        assert_eq!(atlas.edge(), 64);
        assert_eq!(atlas.texture().width(), 16, "no texture until the commit");
        assert!(atlas.has_pending_grow());
        assert_eq!(
            atlas.allocate(U16Vec2::new(40, 40)),
            Some(U16Vec2::new(0, 16)),
            "the packer already places on the grown edge"
        );

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        assert!(atlas.commit(&gpu, &mut encoder));
        assert!(!atlas.commit(&gpu, &mut encoder), "nothing left to commit");
        gpu.queue.submit(iter::once(encoder.finish()));
        assert_eq!(atlas.texture().width(), 64);

        let read = gpu.read_texture(atlas.texture());
        for y in 0..64usize {
            for x in 0..64usize {
                let want = if x < 16 && y < 16 {
                    texels[y * 16 + x]
                } else {
                    0
                };
                assert_eq!(read[y * 64 + x], want, "texel ({x}, {y})");
            }
        }
    }
}
