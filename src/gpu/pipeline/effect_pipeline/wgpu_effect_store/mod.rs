//! The pipelines of the effects a host loaded, built where a use becomes known.

use fixedbitset::FixedBitSet;

use crate::effect::effect_module::EffectModule;
use crate::effect::effect_registry::{ChangesSince, EffectChange, EffectRegistry};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::effect_pipeline::{EffectDevice, EffectTarget};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::EffectKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::template_cover::TemplateCover;

/// The pipelines a backend draws effects with, on its device. It reads the host's
/// [`EffectRegistry`], which holds each effect's module, and builds the pipelines some use wants
/// at one place, the start of a frame's GPU phase ([`Self::prepare`]), before any pass: the box
/// form of an effect a load wants built at once and of each effect a window's themes name, and the
/// form of each draw a frame holds. A theme's effects so build together before a press can reach
/// them, and nothing builds that no use wants, as a pipeline-state cache with a warm-up does
/// (Unity's `ShaderVariantCollection.WarmUp`, Unreal's PSO precaching). Refused: building at the
/// load or the theme set, which a later one replaces for nothing; at the first press, which
/// stutters, the reason Flutter wrote Impeller; on a worker, which shows an effect late; every
/// form at a want, two pipelines where a press and a material draw a box. Cost: an effect's first
/// triangle builds at the frame that draws it.
///
/// Each wanted form builds the half of its [`ClipVariant`] a frame with no rounded clip binds, and
/// the half that takes them from the first frame that draws one, every wanted form at once, as
/// every other family builds a half on the first frame whose census asks for it. Refused: both
/// halves at the want, which compiles a program an app with no rounded clip never binds, at its
/// load and at each device recovery. Cost: that first frame builds the clipped half of every
/// wanted form.
///
/// A wanted form stays built for every format. The store lives as long as its device: one on a
/// new device reads every effect and every want the registry holds, and builds the forms a frame
/// draws as frames draw them. Refused: a store the registry pushes into, shared through an `Rc`,
/// which held a second copy of every module.
#[derive(Debug)]
pub(crate) struct WgpuEffectStore {
    device: EffectDevice,
    /// One for each format, a few in all, in the order each was first drawn to: a frame reads
    /// one format's pipelines whole, and a build adds one to each. Refused: a table of each
    /// effect's pipelines for each format, which a frame reads one entry a row of; one flat table,
    /// which a new format reshapes.
    formats: Vec<FormatEffects>,
    /// Each loaded effect's kind, at its id, which a draw binds by.
    kinds: Vec<KindId>,
    /// Each loaded effect's compiled text, at its id, once a form of it builds: one parse and
    /// validation an effect for every form and format, which costs about what a pipeline does,
    /// dropped at a reload.
    shaders: Vec<Option<wgpu::ShaderModule>>,
    /// The forms some use wanted, at each key's [`EffectKey::slot`]: built for every format.
    wanted: FixedBitSet,
    /// The wanted forms the next [`Self::prepare`] builds: a new want, a want reloaded with a new
    /// module, or every want when the clipped half is first asked for.
    pending: Vec<EffectKey>,
    /// Whether a frame drew rounded clips, from which on every wanted form builds the clipped half
    /// too.
    clipped: bool,
    /// What the next read of the registry hands over: everything, for a store on a new device.
    since: ChangesSince,
}

/// Every pipeline one format draws effects with: each form of each loaded effect,
/// [`ClipVariant::NONE`] where none is built.
#[derive(Debug)]
pub(crate) struct FormatEffects {
    format: wgpu::TextureFormat,
    /// At each key's [`EffectKey::slot`].
    pub(crate) effects: Vec<ClipVariant>,
}

impl WgpuEffectStore {
    /// A store on `gpu`, binding images through `binding`, the device's.
    pub(crate) fn new(gpu: &Gpu, binding: &TextureBinding) -> Self {
        Self {
            device: EffectDevice::new(gpu, binding),
            formats: Vec::new(),
            kinds: Vec::new(),
            shaders: Vec::new(),
            wanted: FixedBitSet::new(),
            pending: Vec::new(),
            clipped: false,
            since: ChangesSince::Start,
        }
    }

    /// Build every wanted form for `format` on `gpu`, this store's device, where it is new: before
    /// the frame that paints it.
    pub(crate) fn ensure_format(
        &mut self,
        gpu: &Gpu,
        effects: &EffectRegistry,
        format: wgpu::TextureFormat,
    ) {
        self.read(effects);
        if self.for_format(format).is_some() {
            return;
        }
        let mut built = FormatEffects {
            format,
            effects: vec![ClipVariant::NONE; self.kinds.len() * EffectKey::FORMS],
        };
        let modules = effects.modules();
        for slot in self.wanted.ones() {
            let key = EffectKey::of_slot(slot);
            let module = modules.get(key.effect);
            self.device.ensure(
                gpu,
                &mut built.effects[slot],
                EffectTarget {
                    module,
                    shader: shader(&mut self.shaders, gpu, key.effect, module),
                    key,
                    format,
                    halves: halves(self.clipped),
                },
            );
        }
        self.formats.push(built);
    }

    /// The start of a frame's GPU phase on `gpu`, this store's device: read what `effects`
    /// changed, want each key of `keys`, the frame's draws, and build every wanted form's halves
    /// not built yet for each format, before any pass; a frame of `clipping` [`Clipping::On`]
    /// first asks for the clipped half of every wanted form.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        effects: &EffectRegistry,
        keys: &[EffectKey],
        clipping: Clipping,
    ) {
        self.read(effects);
        for &key in keys {
            self.want(key);
        }
        if clipping == Clipping::On && !self.clipped {
            self.clipped = true;
            self.pending.clear();
            self.pending
                .extend(self.wanted.ones().map(EffectKey::of_slot));
        }
        if self.pending.is_empty() {
            return;
        }
        let modules = effects.modules();
        for key in self.pending.drain(..) {
            let module = modules.get(key.effect);
            let shader = shader(&mut self.shaders, gpu, key.effect, module);
            for built in &mut self.formats {
                self.device.ensure(
                    gpu,
                    &mut built.effects[key.slot()],
                    EffectTarget {
                        module,
                        shader,
                        key,
                        format: built.format,
                        halves: halves(self.clipped),
                    },
                );
            }
        }
    }

    /// Every pipeline `format` draws effects with; `None` where the format is not built.
    pub(crate) fn for_format(&self, format: wgpu::TextureFormat) -> Option<&FormatEffects> {
        self.formats.iter().find(|built| built.format == format)
    }

    /// Each loaded effect's kind, at its id.
    pub(crate) fn kinds(&self) -> &[KindId] {
        &self.kinds
    }

    /// The layouts every effect's pipeline builds over.
    pub(crate) const fn device(&self) -> &EffectDevice {
        &self.device
    }

    /// Take what `effects` changed since this store last read it: a new effect grows every
    /// format's table, a reloaded one drops its pipelines and builds its wanted forms again, and a
    /// want of a box builds at the next prepare.
    fn read(&mut self, effects: &EffectRegistry) {
        let since = self.since;
        effects.drain_changes(since, |change| match change {
            EffectChange::Loaded { id, module } => self.load(id, module.kind),
            EffectChange::Wanted(id) => self.want(EffectKey {
                effect: id,
                form: ShapeForm::Box,
                cover: TemplateCover::Whole,
            }),
        });
        self.since = ChangesSince::LastRead;
    }

    /// Hold `id`, of `kind`, new or in place of the one before.
    fn load(&mut self, id: EffectId, kind: KindId) {
        debug_assert!(
            id.idx() <= self.kinds.len(),
            "the registry hands out ids in order"
        );
        if id.idx() == self.kinds.len() {
            self.kinds.push(kind);
            self.shaders.push(None);
            let slots = self.kinds.len() * EffectKey::FORMS;
            self.wanted.grow(slots);
            for built in &mut self.formats {
                built.effects.resize(slots, ClipVariant::NONE);
            }
            return;
        }
        self.kinds[id.idx()] = kind;
        self.shaders[id.idx()] = None;
        // Each wanted form of an effect reloaded drops its pipelines, and builds its new module
        // at the next prepare.
        let first = EffectKey::FORMS * id.idx();
        for key in (first..first + EffectKey::FORMS).map(EffectKey::of_slot) {
            for built in &mut self.formats {
                built.effects[key.slot()] = ClipVariant::NONE;
            }
            if self.wanted.contains(key.slot()) && !self.pending.contains(&key) {
                self.pending.push(key);
            }
        }
    }

    /// Want the form `key` names: the next prepare builds it, where no use wanted it before.
    fn want(&mut self, key: EffectKey) {
        if !self.wanted.put(key.slot()) {
            self.pending.push(key);
        }
    }
}

/// The compiled text of `effect`, `module`, from `shaders`, compiling it on `gpu` at its first use.
fn shader<'s>(
    shaders: &'s mut [Option<wgpu::ShaderModule>],
    gpu: &Gpu,
    effect: EffectId,
    module: &EffectModule,
) -> &'s wgpu::ShaderModule {
    shaders[effect.idx()].get_or_insert_with(|| EffectDevice::shader(gpu, effect, module))
}

/// The halves every wanted form builds: the clipless one, and the clipped one once a frame drew
/// rounded clips.
const fn halves(clipped: bool) -> &'static [Clipping] {
    if clipped {
        &[Clipping::Off, Clipping::On]
    } else {
        &[Clipping::Off]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
    use crate::gpu::pipeline::effect_pipeline::wgpu_effect_store::{
        FormatEffects, WgpuEffectStore,
    };
    use crate::primitives::identity::effect_id::EffectId;
    use crate::renderer::render_buffer::effect_draw_row::EffectKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::renderer::render_buffer::template_cover::TemplateCover;

    impl WgpuEffectStore {
        /// Whether each loaded effect's pipeline of `form`, at its id, has the half `clipping`
        /// binds built, the same for every format drawn to.
        pub(crate) fn built_effects(&self, form: ShapeForm, clipping: Clipping) -> Vec<bool> {
            (0..self.kinds.len())
                .map(|idx| {
                    let key = EffectKey {
                        effect: EffectId::from_idx(idx),
                        form,
                        cover: TemplateCover::Whole,
                    };
                    self.built_alike(|format| &format.effects[key.slot()], clipping)
                })
                .collect()
        }

        /// Whether the half `clipping` binds of the pipeline `of` picks is built, which every
        /// format drawn to agrees on.
        fn built_alike(
            &self,
            of: impl Fn(&FormatEffects) -> &ClipVariant,
            clipping: Clipping,
        ) -> bool {
            let formats = &self.formats;
            assert!(!formats.is_empty(), "a frame drew to a format");
            let built = |format| of(format).is_built(clipping);
            let first = built(&formats[0]);
            assert!(formats.iter().all(|format| built(format) == first));
            first
        }
    }
}

#[cfg(test)]
mod tests;
