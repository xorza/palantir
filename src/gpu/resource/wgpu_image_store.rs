//! GPU textures and bind groups for registered images, created, rewritten and
//! freed as the registry asks.

use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::texture_region::TextureRegion;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::renderer::image_registry::image_store::ImageStore;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::array;
use std::cell::{Ref, RefCell};
use std::sync::OnceLock;

/// The [`ImageStore`] a host's registry writes through and its backend draws
/// from. They share one `Rc`, so the map sits behind a `RefCell`; a draw takes
/// one [`Self::read`] per pass.
#[derive(Debug)]
pub(crate) struct WgpuImageStore {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The backend's group-0 layout and sampler, shared with `GpuView` targets.
    binding: TextureBinding,
    textures: RefCell<FxHashMap<TextureId, ImageTexture>>,
    /// Staging for [`premultiply_into`], reused across updates. Opaque images skip
    /// it ([`Self::write`]).
    staged: RefCell<Vec<SrgbaU8>>,
    /// Built at startup: the build takes milliseconds, which shouldn't land on the
    /// frame that registers a soft-edged image.
    premultiply: &'static [[u8; 256]; 256],
}

#[derive(Debug)]
pub(crate) struct ImageTexture {
    texture: wgpu::Texture,
    pub(crate) bind_group: wgpu::BindGroup,
}

impl WgpuImageStore {
    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue, binding: TextureBinding) -> Self {
        Self {
            binding,
            device,
            queue,
            textures: RefCell::default(),
            staged: RefCell::default(),
            premultiply: premultiplied_bytes(),
        }
    }

    /// One borrow per render traversal, avoiding a `RefCell` probe per run.
    pub(crate) fn read(&self) -> Ref<'_, FxHashMap<TextureId, ImageTexture>> {
        self.textures.borrow()
    }

    /// An empty texture at `size` and its bind group; the write that follows fills it.
    fn create(&self, id: TextureId, size: UVec2) -> ImageTexture {
        let raw_id = id.0;
        let texture_label = format!("palantir.image.tex.{raw_id:016x}");
        let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&texture_label),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self
            .binding
            .bind_group(&self.device, &view, &bind_group_label);
        ImageTexture {
            texture,
            bind_group,
        }
    }
}

impl ImageStore for WgpuImageStore {
    fn write(&self, id: TextureId, image: &Image) {
        let mut textures = self.textures.borrow_mut();
        let entry = textures
            .entry(id)
            .or_insert_with(|| self.create(id, image.size));
        debug_assert_eq!(
            UVec2::new(entry.texture.width(), entry.texture.height()),
            image.size,
            "a write matches the texture it lands in",
        );
        let region = TextureRegion {
            texture: &entry.texture,
            first_row: 0,
            size: image.size,
            bytes_per_row: image.size.x * 4,
        };
        // An image with no transparency is already premultiplied (scale by one), so
        // it is written as is rather than copied into a second buffer.
        let texels = image.texels();
        if texels.iter().all(|texel| texel.a == u8::MAX) {
            region.write(&self.queue, &image.pixels);
            return;
        }
        let mut staged = self.staged.borrow_mut();
        premultiply_into(self.premultiply, texels, &mut staged);
        region.write(&self.queue, bytemuck::cast_slice(&staged));
    }

    fn free(&self, id: TextureId) {
        self.textures.borrow_mut().remove(&id);
    }
}

/// `texels` with every colour scaled by its own alpha, still sRGB-encoded:
/// what the texture holds and an [`Image`] does not.
///
/// **The filter runs before the shader**, so texels must already carry their
/// coverage: straight colour blended halfway to transparent black leaves a
/// dark (or coloured) fringe once the shader multiplies by alpha. The image
/// shader's header states the contract.
///
/// The scale is in linear light, which the sRGB texture decodes to and the
/// filter blends.
///
/// The caller tests the alpha, since the error needs differing neighbour
/// alphas, a property of the image. The raster atlases keep straight alpha
/// because their shader reads one texel per pixel and the icon path
/// premultiplies its own taps.
///
/// Paid once per upload; an image refilled every frame pays every frame.
fn premultiply_into(table: &[[u8; 256]; 256], texels: &[SrgbaU8], out: &mut Vec<SrgbaU8>) {
    out.clear();
    out.reserve_exact(texels.len());
    out.extend(texels.iter().map(|texel| {
        let row = &table[texel.a as usize];
        SrgbaU8 {
            r: row[texel.r as usize],
            g: row[texel.g as usize],
            b: row[texel.b as usize],
            a: texel.a,
        }
    }));
}

/// `encode(decode(value) * alpha)` for every byte pair, indexed `[alpha][value]`.
///
/// The arithmetic (an sRGB decode and an iterative encode) is what costs, so
/// it runs 65536 times once instead of per channel per texel. 64 KiB, built
/// with the first image store and kept for the process.
fn premultiplied_bytes() -> &'static [[u8; 256]; 256] {
    static TABLE: OnceLock<Box<[[u8; 256]; 256]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let decoded: [f32; 256] = array::from_fn(|value| {
            RgbaF32::from(SrgbaU8 {
                r: value as u8,
                g: 0,
                b: 0,
                a: u8::MAX,
            })
            .r
        });
        let mut table: Box<[[u8; 256]; 256]> =
            vec![[0u8; 256]; 256].into_boxed_slice().try_into().unwrap();
        for (alpha, row) in table.iter_mut().enumerate() {
            let coverage = alpha as f32 / 255.0;
            for (value, out) in row.iter_mut().enumerate() {
                *out = RgbaF32 {
                    r: decoded[value] * coverage,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }
                .to_srgba_u8()
                .r;
            }
        }
        // Full coverage must reach the GPU as the bytes it was handed, but the
        // round trip drifts by an LSB, so the identity is stated.
        table[usize::from(u8::MAX)] = array::from_fn(|value| value as u8);
        table
    })
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::resource::wgpu_image_store::WgpuImageStore;

    impl WgpuImageStore {
        /// Registered images resident on the GPU; surface-format-change tests assert
        /// it survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.textures.borrow().len()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::texture_binding::TextureBinding;
    use crate::gpu::resource::wgpu_image_store::{
        WgpuImageStore, premultiplied_bytes, premultiply_into,
    };
    use crate::gpu::test_gpu;
    use crate::primitives::identity::texture_id::TextureId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use crate::primitives::paint::image::Image;
    use crate::renderer::image_registry::ImageRegistry;
    use crate::renderer::image_registry::image_handle::ImageHandle;
    use glam::UVec2;
    use std::rc::Rc;

    /// Must match the arithmetic over every byte pair, except full coverage,
    /// which is the identity.
    #[test]
    fn the_premultiply_table_answers_for_the_arithmetic() {
        let texels: Vec<SrgbaU8> = (0..=u8::MAX)
            .flat_map(|a| {
                (0..=u8::MAX).map(move |v| SrgbaU8 {
                    r: v,
                    g: v,
                    b: v,
                    a,
                })
            })
            .collect();
        let mut out = Vec::new();
        premultiply_into(premultiplied_bytes(), &texels, &mut out);

        for (texel, got) in texels.iter().zip(&out) {
            let straight = RgbaF32::from(*texel);
            let want = match texel.a {
                u8::MAX => *texel,
                _ => RgbaF32 {
                    r: straight.r * straight.a,
                    g: straight.g * straight.a,
                    b: straight.b * straight.a,
                    a: straight.a,
                }
                .to_srgba_u8(),
            };
            assert_eq!(*got, want, "premultiplied {texel:?}");
        }
    }

    #[test]
    fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
        let gpu = test_gpu::headless_test_gpu();
        let store = Rc::new(WgpuImageStore::new(
            gpu.device.clone(),
            gpu.queue.clone(),
            TextureBinding::new(&gpu.device),
        ));
        let weak = Rc::downgrade(&store);
        let registry = ImageRegistry::default();
        registry.attach(Rc::clone(&store));
        assert!(!store.read().contains_key(&TextureId(1)));

        let mut image = Image::blank(UVec2::splat(2));
        let handle = ImageHandle::new(TextureId(1), &image, registry.clone());
        assert_eq!(store.resident(), 1);
        let registered = store.read()[&TextureId(1)].bind_group.clone();
        assert!(!store.read().contains_key(&TextureId(2)));
        image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
        handle.update(&image);
        assert_eq!(handle.generation(), 1);
        assert_eq!(
            store.read()[&TextureId(1)].bind_group,
            registered,
            "a write keeps the texture and its binding",
        );
        let clone = handle.clone();
        drop(handle);
        assert_eq!(store.resident(), 1, "a clone keeps the texture");
        drop(clone);
        assert_eq!(store.resident(), 0);
        assert!(!store.read().contains_key(&TextureId(1)));

        let survivor = ImageHandle::new(TextureId(2), &image, registry);
        drop(store);
        survivor.update(&image);
        assert_eq!(survivor.generation(), 1);
        assert_eq!(weak.upgrade().unwrap().resident(), 1);
        drop(survivor);
        assert!(
            weak.upgrade().is_none(),
            "the last image handle releases a store whose other owners are gone",
        );
    }
}
