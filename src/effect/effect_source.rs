//! [`EffectSource`]: what [`Ui::load_effect`](crate::Ui::load_effect) loads under a name.

use std::borrow::Cow;
use std::fmt;

use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};
use crate::primitives::effect::effect_kind::{ColorEffect, EffectKind};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::paint::material::material_reach::MaterialReach;

/// An effect's WGSL, the image slots its shapes bind, and what paints while motion or power use is
/// reduced. What a backdrop effect paints while transparency is reduced is each use's
/// ([`MaterialFallback`](crate::MaterialFallback)).
/// Consumed by [`Ui::load_effect`](crate::Ui::load_effect).
///
/// The WGSL declares `fn effect(input: EffectInput, params: Params) -> EffectOutput`, where
/// `Params` is the struct `U` describes, or `fn effect(input: EffectInput) -> EffectOutput` for an
/// effect with no parameters (`U` of no bytes, as `()`). `effect` is a plain function, with no
/// stage attribute. Palantir declares, after the app's text, so a parse error's position reads in
/// the app's own text:
///
/// - `EffectInput { pos, size, corners, time, scale }`: the pixel's center from the rect's
///   top-left, the rect's size and its corner radii in logical px, the seconds into the pass of
///   the shape's [`PaintAnimation`](crate::widget::PaintAnimation), 0 without one, and physical px
///   per logical px;
/// - `EffectOutput { color, under }`: `color + under · dst` per channel, linear and
///   premultiplied, with the shape's coverage applied exactly, as the blend is linear in it;
/// - `effect_over`, `effect_add`, `effect_multiply` and `effect_screen`, each an
///   `EffectOutput` from a premultiplied color;
/// - with [image slots](Self::image_slots), `effect_image0` and on, each a `texture_2d<f32>` of
///   the image its paint binds in that slot ([`EffectPaint::images`](crate::widget::EffectPaint::images)),
///   whose sample is linear and premultiplied, and `effect_sampler`, linear and clamped to the
///   edge, as `textureSample(effect_image0, effect_sampler, uv)` reads them;
/// - for a [`BackdropEffect`](crate::BackdropEffect), `effect_backdrop(pos)`: what is painted
///   under the shape at `pos`, logical px from its rect's top-left, linear and opaque, blurred by
///   its paint's [σ](crate::widget::EffectPaint::blur), and read within its
///   [`max_sample_offset`](crate::widget::EffectPaint::max_sample_offset), a read past it
///   reading its edge. A sample that needs no uniform control flow, so a loop or a branch may
///   read it.
///
/// The WGSL declares no binding of its own, as palantir binds every resource an effect reads;
/// one is [`EffectError::Binding`](crate::EffectError::Binding).
///
/// Palantir builds the real stages around `effect`, as SwiftUI's stitchable functions and Skia's
/// SkSL `main` are wrapped, so an effect stays correct under partial repaint and clips. Names that
/// start with `palantir_` are palantir's. A blend that is not linear in what is under it is a
/// backdrop effect. Refused: a whole vertex and fragment module (Qt's `ShaderEffect`, Godot),
/// which leaves each app's shader to keep palantir's coverage, clip and output rules.
/// Plain WGSL: an app that wants modules runs WESL or naga_oil itself, as both emit WGSL. Refused:
/// either inside palantir, a public dependency every app pays for.
#[must_use]
pub struct EffectSource<U: EffectParams, K: EffectKind = ColorEffect> {
    pub(crate) wgsl: Cow<'static, str>,
    pub(crate) image_slots: u8,
    pub(crate) reduced_motion: ReducedPaint<U, K>,
    pub(crate) power_saver: ReducedPaint<U, K>,
    pub(crate) build: EffectBuild,
    pub(crate) reach: fn(&U) -> MaterialReach,
}

/// When a loaded effect builds its pipelines. Every build runs at the start of a frame's GPU phase,
/// before any pass; this picks the frame. A theme that names the effect builds it with its other
/// effects at the next frame that draws after the theme is set
/// ([`Ui::set_theme`](crate::Ui::set_theme)), whichever is picked here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EffectBuild {
    /// At the first frame that draws it, or after a theme that names it is set; an effect no use
    /// reaches builds nothing. The first frame that draws it with no theme naming it builds it
    /// then, which a frame can show as a stutter.
    #[default]
    OnUse,
    /// At the next frame, whether anything draws it yet: for an effect a shape draws at a moment
    /// where a build would show.
    Now,
}

impl<U: EffectParams, K: EffectKind> EffectSource<U, K> {
    /// The most image slots an effect binds, as GL and older Metal tiers lack a bindless array.
    pub const MAX_IMAGE_SLOTS: u8 = MAX_IMAGE_SLOTS;
    /// The most bytes `U` holds, so one storage buffer row serves any effect's instance.
    pub const MAX_PARAMS_SIZE: usize = MAX_PARAMS_SIZE;

    /// An effect of `wgsl` with no image slots, which paints the same whatever is reduced and
    /// builds on use.
    pub fn new(wgsl: impl Into<Cow<'static, str>>) -> Self {
        Self {
            wgsl: wgsl.into(),
            image_slots: 0,
            reduced_motion: ReducedPaint::Same,
            power_saver: ReducedPaint::Same,
            build: EffectBuild::OnUse,
            reach: |_| MaterialReach::NONE,
        }
    }

    /// How far past its box a material over the effect reads, from the material's parameters:
    /// [`MaterialReach::NONE`] by default, its box alone. How far a ray bends is the effect's, so
    /// each material over it reads as far as its parameters bend, as the stock glass reads as far
    /// as its lens. An [`EffectPaint`](crate::widget::EffectPaint) names its own
    /// ([`EffectPaint::max_sample_offset`](crate::widget::EffectPaint::max_sample_offset)).
    pub const fn reach(mut self, reach: fn(&U) -> MaterialReach) -> Self {
        self.reach = reach;
        self
    }

    /// When the effect builds its pipelines; [`EffectBuild::OnUse`] by default.
    pub const fn build(mut self, build: EffectBuild) -> Self {
        self.build = build;
        self
    }

    /// How many images each paint of the effect binds, at most [`Self::MAX_IMAGE_SLOTS`]; a load
    /// past it returns [`EffectError::TooManyImageSlots`](crate::EffectError::TooManyImageSlots).
    /// Each paint binds exactly this many, so a draw binds one group of them, which splits a run
    /// of draws only where neighbors bind other images.
    pub const fn image_slots(mut self, slots: u8) -> Self {
        self.image_slots = slots;
        self
    }

    /// What paints in place of the effect while motion is reduced
    /// ([`Ui::is_motion_reduced`](crate::Ui::is_motion_reduced)); [`ReducedPaint::Same`] by
    /// default, where the effect's paint animation alone holds still, as
    /// [`MotionRole`](crate::widget::MotionRole) says. It wins over [`Self::power_saver`], as what
    /// a person needs to bear comes before what a device saves.
    pub const fn reduced_motion(mut self, paint: ReducedPaint<U, K>) -> Self {
        self.reduced_motion = paint;
        self
    }

    /// What paints in place of the effect while power is reduced
    /// ([`Ui::is_power_reduced`](crate::Ui::is_power_reduced)); [`ReducedPaint::Same`] by
    /// default.
    pub const fn power_saver(mut self, paint: ReducedPaint<U, K>) -> Self {
        self.power_saver = paint;
        self
    }

    /// Each alternative, in the order one wins over the next: motion, power.
    pub(crate) const fn alternatives(&self) -> [ReducedPaint<U, K>; 2] {
        [self.reduced_motion, self.power_saver]
    }
}

/// What paints in place of an effect while the system asks to reduce motion or power use, carried
/// by its source ([`EffectSource::reduced_motion`], [`EffectSource::power_saver`]): each swaps the
/// effect for a cheaper or stiller one, which the effect itself knows of. A paint resolves it as it
/// is added, and a move of either preference records every window again. An alternative paints as
/// itself: its own alternatives do not apply. Refused: a theme slot for each mode, which doubles
/// every slot; a branch at each call site; transparency among them, as what a backdrop shows while
/// it is reduced depends on each use's role ([`MaterialFallback`](crate::MaterialFallback)).
pub enum ReducedPaint<U: EffectParams, K: EffectKind = ColorEffect> {
    /// The effect itself, as cheap enough already.
    Same,
    /// Another effect with the same parameters and kind, loaded by the time it paints.
    Effect(EffectName<U, K>),
    /// Nothing paints in its place.
    Nothing,
}

impl<U: EffectParams, K: EffectKind> Clone for ReducedPaint<U, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U: EffectParams, K: EffectKind> Copy for ReducedPaint<U, K> {}

impl<U: EffectParams, K: EffectKind> PartialEq for ReducedPaint<U, K> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Same, Self::Same) | (Self::Nothing, Self::Nothing) => true,
            (Self::Effect(a), Self::Effect(b)) => a == b,
            _ => false,
        }
    }
}

impl<U: EffectParams, K: EffectKind> Eq for ReducedPaint<U, K> {}

impl<U: EffectParams, K: EffectKind> fmt::Debug for ReducedPaint<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Same => f.write_str("Same"),
            Self::Effect(name) => f.debug_tuple("Effect").field(name).finish(),
            Self::Nothing => f.write_str("Nothing"),
        }
    }
}

impl<U: EffectParams, K: EffectKind> fmt::Debug for EffectSource<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectSource")
            .field("wgsl", &self.wgsl)
            .field("image_slots", &self.image_slots)
            .field("reduced_motion", &self.reduced_motion)
            .field("power_saver", &self.power_saver)
            .field("build", &self.build)
            .finish_non_exhaustive()
    }
}
