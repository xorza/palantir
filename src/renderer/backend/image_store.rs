//! The GPU side of registered images: their textures and bind groups,
//! created, rewritten and freed the moment the registry asks.

use crate::primitives::color::RgbaF32;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::image_binding::ImageBinding;
use crate::renderer::backend::texture_region::TextureRegion;
use crate::renderer::image_registry::image_store::ImageStore;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::cell::{Ref, RefCell};
use std::sync::OnceLock;

/// The [`ImageStore`] a host's registry writes through and its backend
/// draws from. The two hold one `Rc` between them, which is why the map
/// sits behind a `RefCell`: a handle creates, rewrites or frees through a
/// shared reference between frames, and the draw takes one [`Self::read`]
/// for a whole pass. Only a queue drained by the backend under `&mut self`
/// would remove the cell, and that is the staged-upload design this
/// immediate one replaced.
#[derive(Debug)]
pub(super) struct WgpuImageStore {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The group-0 layout and sampler every image bind group is built
    /// against. The `GpuView` targets clone it, so a composite of a view
    /// binds exactly like an image, and each format's image pipeline
    /// composes over its layout.
    binding: ImageBinding,
    textures: RefCell<FxHashMap<TextureId, ImageTexture>>,
    /// Where [`premultiply_into`] stages a write, kept so a refilled
    /// image allocates once rather than once per update. An image with
    /// no transparency never reaches it — see [`Self::write`].
    staged: RefCell<Vec<SrgbaU8>>,
}

#[derive(Debug)]
pub(super) struct ImageTexture {
    texture: wgpu::Texture,
    pub(super) bind_group: wgpu::BindGroup,
}

impl WgpuImageStore {
    pub(super) fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            binding: ImageBinding::new(&device),
            device,
            queue,
            textures: RefCell::default(),
            staged: RefCell::default(),
        }
    }

    pub(super) fn binding(&self) -> &ImageBinding {
        &self.binding
    }

    /// One borrow for a render traversal, so a draw pays neither a
    /// `RefCell` probe nor a handle clone per run.
    pub(super) fn read(&self) -> Ref<'_, FxHashMap<TextureId, ImageTexture>> {
        self.textures.borrow()
    }

    /// An empty texture at `size` and the bind group a draw samples it
    /// through. The texels follow in the write that asked for it.
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
        let view = texture.create_view(&Default::default());
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
        // An image with nothing transparent in it is premultiplied
        // already — every colour is scaled by one — so it goes as it
        // stands, for a read the write pays anyway rather than a second
        // buffer the size of the image. A photograph, a screenshot and
        // an opaque generated surface all take that path: what pays is
        // what could have had a fringe.
        let texels = image.texels();
        if texels.iter().all(|texel| texel.a == u8::MAX) {
            region.write(&self.queue, &image.pixels);
            return;
        }
        let mut staged = self.staged.borrow_mut();
        premultiply_into(texels, &mut staged);
        region.write(&self.queue, bytemuck::cast_slice(&staged));
    }

    fn free(&self, id: TextureId) {
        self.textures.borrow_mut().remove(&id);
    }
}

/// `texels` with every colour scaled by its own alpha, still sRGB-encoded
/// — what the texture holds, and what an [`Image`] does not.
///
/// **The filter runs before the shader does**, so the texels it blends
/// have to carry their own coverage already: halfway between opaque red
/// and transparent black, straight colour averages to full red at half
/// alpha, and the shader's multiply then leaves a quarter of the red the
/// edge covers. That is a dark fringe around every soft edge, and a
/// coloured one wherever the clear texels carry a colour. The image
/// shader's header states the contract this satisfies.
///
/// The scale happens in linear light, because that is what the sRGB
/// texture decodes to and what the filter blends.
///
/// **A texture cannot follow how it is sampled.** The error needs a
/// blending filter *and* neighbours whose alpha differs, and only the
/// second is a property of the image — one texture serves every draw of
/// it, magnified here and snapped to its texels there. So the caller
/// tests the alpha, and this converts whatever it is handed. The raster
/// atlases are the textures that *can* answer the first, and they answer
/// it the other way: one texel per pixel with `Nearest`, and straight
/// alpha kept, which is what lets the icon rasterizer hand them
/// demultiplied pixels.
///
/// Paid once per upload rather than per fragment, which suits an image
/// registered once and sampled for as long as it is shown. An
/// application refilling one every frame pays it every frame.
fn premultiply_into(texels: &[SrgbaU8], out: &mut Vec<SrgbaU8>) {
    let table = premultiplied_bytes();
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

/// `encode(decode(value) · alpha)` for every byte pair a texel can hold,
/// indexed `[alpha][value]`.
///
/// **The arithmetic is what costs, not the pass.** One entry is a cubic
/// sRGB decode and an encode that seeds with `powf` and then runs three
/// Newton steps — around a hundred nanoseconds, per channel, per texel. A
/// soft-edged image is mostly part-transparent texels, so computing it
/// per texel would spend that three times over on every one of them and
/// stall the frame that registered the image. Two lookups instead, and
/// the pass over the buffer is what is left.
///
/// 64 KiB, built on the first upload and kept for the process. The build
/// spends the arithmetic 65536 times, which is less than one soft image
/// of any size would have spent.
fn premultiplied_bytes() -> &'static [[u8; 256]; 256] {
    static TABLE: OnceLock<Box<[[u8; 256]; 256]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let decoded: [f32; 256] = std::array::from_fn(|value| {
            RgbaF32::from(SrgbaU8 {
                r: value as u8,
                g: 0,
                b: 0,
                a: u8::MAX,
            })
            .r
        });
        let mut table = Box::new([[0u8; 256]; 256]);
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
        // Full coverage scales by one, and an opaque image must reach the
        // GPU as the bytes it was handed. The decode and encode either
        // side of that multiply round-trip to within one LSB rather than
        // exactly, so the identity is stated instead of computed.
        table[usize::from(u8::MAX)] = std::array::from_fn(|value| value as u8);
        table
    })
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::renderer::backend::image_store::WgpuImageStore;

    impl WgpuImageStore {
        /// Registered images resident on the GPU. The surface-format-change
        /// tests assert this survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.textures.borrow().len()
        }
    }
}

#[cfg(all(test, feature = "internals"))]
mod tests {
    use crate::host::test_gpu;
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::srgba_u8::SrgbaU8;
    use crate::primitives::image::Image;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::backend::image_store::{WgpuImageStore, premultiply_into};
    use crate::renderer::image_registry::ImageRegistry;
    use crate::renderer::image_registry::image_handle::ImageHandle;
    use glam::UVec2;
    use std::rc::Rc;

    /// The table stands in for the arithmetic, so it answers what the
    /// arithmetic answers — over every byte pair a texel can hold, not a
    /// sample of them. Full coverage is the one deliberate difference:
    /// it is the identity, where the round trip through the transfer
    /// functions could drift a bit.
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
        premultiply_into(&texels, &mut out);

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
        let store = Rc::new(WgpuImageStore::new(gpu.device.clone(), gpu.queue.clone()));
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
