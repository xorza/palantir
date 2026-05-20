//! Glyph atlas: one struct for both mask + color content.

use cosmic_text::CacheKey;
use etagere::{AllocId, BucketedAtlasAllocator, size2};
use rustc_hash::FxHashMap;

use super::ContentType;

/// Initial atlas side length. Bumped from glyphon's 256 to skip the
/// 256→512→1024 grow chain on first frame with non-trivial text.
pub(crate) const INITIAL_ATLAS_SIZE: u32 = 1024;
pub(crate) const ATLAS_GROWTH_FACTOR: u32 = 2;

#[derive(Clone, Copy)]
pub(crate) struct GlyphSlot {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) left: i16,
    pub(crate) top: i16,
    pub(crate) content: ContentType,
    pub(crate) alloc: Option<AllocId>,
    pub(crate) last_use: u64,
}

/// One per-content-type backing store. Indexed by `ContentType as usize`.
pub(crate) struct Side {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) size: u32,
    pub(crate) packer: BucketedAtlasAllocator,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) bpp: u32,
    pub(crate) label: &'static str,
}

pub(crate) struct GlyphAtlas {
    pub(crate) sides: [Side; 2],
    pub(crate) cache: FxHashMap<CacheKey, GlyphSlot>,
    pub(crate) current_frame: u64,
    pub(crate) max_texture_dimension_2d: u32,
    /// Set on grow; the renderer rebuilds its bind group and clears it.
    pub(crate) bind_group_dirty: bool,
}

impl GlyphAtlas {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let max = device.limits().max_texture_dimension_2d;
        let size = INITIAL_ATLAS_SIZE.min(max);

        // Order matches `ContentType as usize`: [Mask, Color].
        let sides = [
            Side::new(
                device,
                size,
                wgpu::TextureFormat::R8Unorm,
                1,
                "palantir text mask atlas",
            ),
            Side::new(
                device,
                size,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                4,
                "palantir text color atlas",
            ),
        ];

        Self {
            sides,
            cache: FxHashMap::default(),
            current_frame: 1,
            max_texture_dimension_2d: max,
            bind_group_dirty: false,
        }
    }

    pub(crate) fn mask_view(&self) -> &wgpu::TextureView {
        &self.sides[ContentType::Mask as usize].view
    }
    pub(crate) fn color_view(&self) -> &wgpu::TextureView {
        &self.sides[ContentType::Color as usize].view
    }
    pub(crate) fn mask_size(&self) -> u32 {
        self.sides[ContentType::Mask as usize].size
    }
    pub(crate) fn color_size(&self) -> u32 {
        self.sides[ContentType::Color as usize].size
    }

    /// Cache-hit fast path.
    pub(crate) fn touch(&mut self, key: &CacheKey) -> Option<GlyphSlot> {
        let slot = self.cache.get_mut(key)?;
        slot.last_use = self.current_frame;
        Some(*slot)
    }

    /// Insert a freshly-rasterized glyph. Uploads via
    /// `queue.write_texture` (wgpu batches internally). Grows if
    /// full; returns `None` only at GPU-max and still doesn't fit.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: CacheKey,
        content: ContentType,
        width: u16,
        height: u16,
        left: i16,
        top: i16,
        pixels: &[u8],
    ) -> Option<GlyphSlot> {
        let alloc = self.allocate(device, queue, content, width, height)?;
        let side = &self.sides[content as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &side.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: alloc.rectangle.min.x as u32,
                    y: alloc.rectangle.min.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width as u32 * side.bpp),
                rows_per_image: Some(height as u32),
            },
            wgpu::Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
        );

        let slot = GlyphSlot {
            x: alloc.rectangle.min.x as u16,
            y: alloc.rectangle.min.y as u16,
            width,
            height,
            left,
            top,
            content,
            alloc: Some(alloc.id),
            last_use: self.current_frame,
        };
        self.cache.insert(key, slot);
        Some(slot)
    }

    /// Insert a zero-area glyph entry (no atlas slot, no upload).
    /// Subsequent lookups still hit the cache and skip swash.
    pub(crate) fn insert_empty(
        &mut self,
        key: CacheKey,
        content: ContentType,
        left: i16,
        top: i16,
    ) -> GlyphSlot {
        let slot = GlyphSlot {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            left,
            top,
            content,
            alloc: None,
            last_use: self.current_frame,
        };
        self.cache.insert(key, slot);
        slot
    }

    /// Frame teardown: bump LRU counter.
    pub(crate) fn trim(&mut self) {
        self.current_frame += 1;
    }

    /// Allocate a slot in the right packer, evicting then growing as
    /// needed.
    fn allocate(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        content: ContentType,
        width: u16,
        height: u16,
    ) -> Option<etagere::Allocation> {
        let need = size2(width as i32, height as i32);
        loop {
            if let Some(a) = self.sides[content as usize].packer.allocate(need) {
                return Some(a);
            }
            if !self.evict_one(content) && !self.grow(device, queue, content) {
                return None;
            }
        }
    }

    /// Evict any glyph of `target` content with `last_use <
    /// current_frame`. Linear scan; eviction is rare in practice.
    fn evict_one(&mut self, target: ContentType) -> bool {
        let cf = self.current_frame;
        let Some(key) = self.cache.iter().find_map(|(k, s)| {
            (s.content == target && s.last_use < cf && s.alloc.is_some()).then_some(*k)
        }) else {
            return false;
        };
        let slot = self.cache.remove(&key).unwrap();
        if let Some(id) = slot.alloc {
            self.sides[target as usize].packer.deallocate(id);
        }
        true
    }

    /// Double the atlas of `content`. Returns `false` at GPU-max. On
    /// success, blits old → new (etagere preserves rects on
    /// `packer.grow`, so the cache stays valid — no re-rasterization).
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, content: ContentType) -> bool {
        let side = &mut self.sides[content as usize];
        if side.size >= self.max_texture_dimension_2d {
            return false;
        }
        let new_size = (side.size * ATLAS_GROWTH_FACTOR).min(self.max_texture_dimension_2d);
        let new_texture = make_texture(device, side.format, new_size, side.label);

        let old_texture = std::mem::replace(&mut side.texture, new_texture);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("palantir text atlas grow"),
        });
        encoder.copy_texture_to_texture(
            old_texture.as_image_copy(),
            side.texture.as_image_copy(),
            wgpu::Extent3d {
                width: side.size,
                height: side.size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        side.view = side
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        side.size = new_size;
        side.packer.grow(size2(new_size as i32, new_size as i32));
        self.bind_group_dirty = true;
        true
    }
}

impl Side {
    fn new(
        device: &wgpu::Device,
        size: u32,
        format: wgpu::TextureFormat,
        bpp: u32,
        label: &'static str,
    ) -> Self {
        let texture = make_texture(device, format, size, label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            format,
            bpp,
            label,
        }
    }
}

fn make_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
