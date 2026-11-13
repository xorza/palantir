//! GPU textures and bind groups for registered images, created, rewritten and
//! freed as the registry asks.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::effect_image_groups::{EffectImageGroups, EffectImages};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::texel;
use crate::gpu::texel::TargetSpec;
use crate::image_registry::error::ImageTooLarge;
use crate::image_registry::image_store::ImageStore;
use crate::image_registry::texture_limit::TextureLimit;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::array;
use std::cell::{Ref, RefCell, RefMut};
use std::ops::Range;

/// The [`ImageStore`] a host's registry writes through and its backend draws
/// from. They share one `Rc`, so the device side sits behind a `RefCell`; a draw takes
/// one [`Self::read`] per pass.
///
/// Built before any device, as the registry every recorder clones holds it from the start; the
/// host [attaches](Self::attach) each device it draws with, before any image registers.
#[derive(Debug)]
pub(crate) struct WgpuImageStore {
    /// `None` until the first attach.
    attached: RefCell<Option<AttachedStore>>,
    /// Built with the store, at startup: the build takes milliseconds, which shouldn't
    /// land on the frame that registers a soft-edged image.
    premultiply: Box<[[u8; 256]; 256]>,
}

/// The store's side of one device: the textures it holds on it.
#[derive(Debug)]
struct AttachedStore {
    gpu: Gpu,
    /// The device's group-0 layout and sampler, which an image's group binds through.
    binding: TextureBinding,
    textures: FxHashMap<TextureId, ImageTexture>,
    /// The groups effect draws bind these textures through.
    effect_groups: EffectImageGroups,
    /// Staging for [`premultiply_into`], reused across updates and held to [`STAGED_BYTES`]: a
    /// translucent image is premultiplied and written a band of rows at a time. Opaque images skip
    /// it ([`WgpuImageStore::write`]).
    staged: Vec<SrgbaU8>,
}

#[derive(Debug)]
pub(crate) struct ImageTexture {
    texture: wgpu::Texture,
    /// The whole texture, which `bind_group` and each effect's image group bind.
    view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
}

const ATTACHED: &str = "the host attaches a device before it draws or registers an image";

/// The most [`AttachedStore::staged`] holds, a band of rows at least one high: a 64 MP
/// translucent image staged whole would keep 256 MiB for the session, and in bands of 1 MiB it
/// costs the same copies, one `write_texture` each. It bounds the CPU copy alone: wgpu stages each
/// write in a buffer of its own until the submit, so the upload's staging peak is the image.
const STAGED_BYTES: u32 = 1 << 20;

/// A store with no device yet.
impl Default for WgpuImageStore {
    fn default() -> Self {
        Self {
            attached: RefCell::default(),
            premultiply: premultiplied_bytes(),
        }
    }
}

impl WgpuImageStore {
    /// Hold textures on `gpu` from now on, bound through `binding`, the device's. The textures
    /// of an earlier device drop: every image paints nothing until its next write.
    pub(crate) fn attach(&self, gpu: &Gpu, binding: TextureBinding) {
        *self.attached.borrow_mut() = Some(AttachedStore {
            gpu: gpu.clone(),
            binding,
            textures: FxHashMap::default(),
            effect_groups: EffectImageGroups::default(),
            staged: Vec::new(),
        });
    }

    /// Note that display frame `frame` draws each set of `images`, making the group of each set
    /// first drawn, before the pass that binds them.
    pub(crate) fn ensure_effect_groups(
        &self,
        images: impl IntoIterator<Item = EffectImages>,
        frame: u64,
    ) {
        let mut attached = self.attached_mut();
        let AttachedStore {
            gpu,
            binding,
            textures,
            effect_groups,
            staged: _,
        } = &mut *attached;
        for set in images {
            effect_groups.ensure(set, frame, || {
                let views: Option<Vec<&wgpu::TextureView>> = set
                    .ids()
                    .iter()
                    .map(|id| textures.get(id).map(|texture| &texture.view))
                    .collect();
                Some(binding.images_group(gpu, &views?))
            });
        }
    }

    /// The groups effect draws bind their images through, one borrow per render traversal.
    pub(crate) fn read_effect_groups(&self) -> Ref<'_, EffectImageGroups> {
        Ref::map(self.attached(), |attached| &attached.effect_groups)
    }

    /// One borrow per render traversal, avoiding a `RefCell` probe per run.
    pub(crate) fn read(&self) -> Ref<'_, FxHashMap<TextureId, ImageTexture>> {
        Ref::map(self.attached(), |attached| &attached.textures)
    }

    fn attached(&self) -> Ref<'_, AttachedStore> {
        Ref::map(self.attached.borrow(), |attached| {
            attached.as_ref().expect(ATTACHED)
        })
    }

    fn attached_mut(&self) -> RefMut<'_, AttachedStore> {
        RefMut::map(self.attached.borrow_mut(), |attached| {
            attached.as_mut().expect(ATTACHED)
        })
    }
}

/// An empty texture at `size` and its bind group; the write that follows fills it.
fn create(gpu: &Gpu, binding: &TextureBinding, id: TextureId, size: UVec2) -> ImageTexture {
    let raw_id = id.0;
    let texture_label = format!("palantir.image.tex.{raw_id:016x}");
    let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
    let texture = texel::texture_2d(
        gpu,
        TargetSpec {
            label: &texture_label,
            size,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = binding.bind_group(gpu, &view, &bind_group_label);
    ImageTexture {
        texture,
        view,
        bind_group,
    }
}

impl ImageStore for WgpuImageStore {
    fn write(&self, id: TextureId, image: &Image) -> Result<(), ImageTooLarge> {
        let mut attached = self.attached_mut();
        // Registration checks the device it ran on; a later device can be smaller.
        TextureLimit::from_device(attached.gpu.max_texture_dim()).accepts(image.size)?;
        let AttachedStore {
            gpu,
            binding,
            textures,
            effect_groups: _,
            staged,
        } = &mut *attached;
        let entry = textures
            .entry(id)
            .or_insert_with(|| create(gpu, binding, id, image.size));
        debug_assert_eq!(
            texel::size(&entry.texture),
            image.size,
            "a write matches the texture it lands in",
        );
        // A pitch off `COPY_BYTES_PER_ROW_ALIGNMENT` is re-packed inside wgpu; padding the rows here
        // would not save that, as wgpu copies the whole into staging either way.
        let row_bytes = image.size.x * 4;
        let write = |y: u32, data: &[u8]| {
            let rows = data.len() as u32 / row_bytes;
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &entry.texture,
                    mip_level: 0,
                    origin: texel::origin(UVec2::new(0, y)),
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(rows),
                },
                texel::extent(UVec2::new(image.size.x, rows)),
            );
        };
        // An image with no transparency is already premultiplied (scale by one), so
        // it is written as is rather than copied into a second buffer.
        let texels = image.texels();
        if texels.iter().all(|texel| texel.a == u8::MAX) {
            write(0, &image.pixels);
            return Ok(());
        }
        let width = image.size.x as usize;
        for rows in bands(image.size) {
            let band = &texels[rows.start as usize * width..rows.end as usize * width];
            premultiply_into(&self.premultiply, band, staged);
            write(rows.start, bytemuck::cast_slice(staged));
        }
        Ok(())
    }

    fn holds(&self, id: TextureId) -> bool {
        self.attached().textures.contains_key(&id)
    }

    fn free(&self, id: TextureId) {
        let mut attached = self.attached_mut();
        attached.textures.remove(&id);
        attached.effect_groups.forget(id);
    }
}

/// The rows of a `size` image, in bands of at most [`STAGED_BYTES`], each at least one row high.
fn bands(size: UVec2) -> impl Iterator<Item = Range<u32>> {
    let band = (STAGED_BYTES / (size.x * 4)).max(1);
    (0..size.y)
        .step_by(band as usize)
        .map(move |start| start..(start + band).min(size.y))
}

/// `texels` with every color scaled by its own alpha, still sRGB-encoded:
/// what the texture holds and an [`Image`] does not.
///
/// The filter runs before the shader, so texels must already carry their coverage: straight color
/// blended halfway to transparent black leaves a dark or colored fringe once the shader multiplies
/// by alpha. The scale is in linear light, which the sRGB texture decodes to and the filter blends.
///
/// The caller tests the alpha, since the error needs differing neighbor alphas, a property of the
/// image. The raster atlases keep straight alpha because their shader reads one texel per pixel and
/// the icon path premultiplies its own taps.
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
/// it runs 65536 times once instead of per channel per texel. 64 KiB, one per
/// store.
fn premultiplied_bytes() -> Box<[[u8; 256]; 256]> {
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
            .to_srgba8()
            .r;
        }
    }
    // Full coverage must reach the GPU as the bytes it was handed, but the
    // round trip drifts by an LSB, so the identity is stated.
    table[usize::from(u8::MAX)] = array::from_fn(|value| value as u8);
    table
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::resource::wgpu_image_store::WgpuImageStore;

    impl WgpuImageStore {
        /// Registered images resident on the GPU; surface-format-change tests assert
        /// it survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.read().len()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::texture_binding::TextureBinding;
    use crate::gpu::resource::wgpu_image_store::{
        STAGED_BYTES, WgpuImageStore, bands, premultiplied_bytes, premultiply_into,
    };
    use crate::gpu::test_gpu;
    use crate::image_registry::ImageRegistry;
    use crate::image_registry::image_handle::ImageHandle;
    use crate::image_registry::image_store::ImageStore;
    use crate::primitives::identity::texture_id::TextureId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use crate::primitives::paint::image::Image;
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
        premultiply_into(&premultiplied_bytes(), &texels, &mut out);

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
                .to_srgba8(),
            };
            assert_eq!(*got, want, "premultiplied {texel:?}");
        }
    }

    #[test]
    fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
        let gpu = test_gpu::headless_test_gpu();
        let store = Rc::new(WgpuImageStore::default());
        store.attach(&gpu, TextureBinding::new(&gpu));
        let weak = Rc::downgrade(&store);
        let registry = ImageRegistry::new(Rc::clone(&store));
        assert!(!store.read().contains_key(&TextureId(1)));

        let mut image = Image::blank(UVec2::splat(2));
        let handle = ImageHandle::new(TextureId(1), &image, registry.clone());
        assert_eq!(store.resident(), 1);
        let registered = store.read()[&TextureId(1)].bind_group.clone();
        assert!(!store.read().contains_key(&TextureId(2)));
        image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
        handle.update(&image).unwrap();
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
        let gpu = test_gpu::headless_test_gpu();
        store.attach(&gpu, TextureBinding::new(&gpu));
        assert_eq!(
            store.resident(),
            0,
            "an attach drops the textures of the old device"
        );
        assert!(!survivor.is_resident());
        survivor.update(&image).unwrap();
        assert_eq!(store.resident(), 1, "a write fills the image again");
        assert!(survivor.is_resident());
        drop(store);
        survivor.update(&image).unwrap();
        assert_eq!(survivor.generation(), 2);
        assert_eq!(weak.upgrade().unwrap().resident(), 1);
        drop(survivor);
        assert!(
            weak.upgrade().is_none(),
            "the last image handle releases a store whose other owners are gone",
        );
    }

    /// A translucent image is premultiplied and written a band of rows at a time, so the staging
    /// never holds more than one: 1024 × 300 texels are 1.2 MiB, in bands of
    /// `2^20 / 4096 = 256` rows; a 16 384-wide image takes `2^20 / 65 536 = 16` rows a band, and an
    /// image under one band writes in one.
    #[test]
    fn a_translucent_image_writes_in_bands_of_the_staging() {
        let bands = |w, h| bands(UVec2::new(w, h)).collect::<Vec<_>>();
        assert_eq!(bands(1024, 300), [0..256, 256..300]);
        assert_eq!(bands(16384, 40), [0..16, 16..32, 32..40]);
        assert_eq!(bands(8, 8).len(), 1, "one band");
        assert_eq!(bands(8, 8).first(), Some(&(0..8)));

        let gpu = test_gpu::headless_test_gpu();
        let store = WgpuImageStore::default();
        store.attach(&gpu, TextureBinding::new(&gpu));
        let mut image = Image::blank(UVec2::new(1024, 300));
        image.texels_mut().fill(SrgbaU8 {
            a: 128,
            ..SrgbaU8::hex(0x4cd3ff)
        });
        store.write(TextureId(1), &image).unwrap();
        let staged = store.attached().staged.capacity() * 4;
        assert!(staged <= STAGED_BYTES as usize, "{staged} bytes staged");
    }
}
