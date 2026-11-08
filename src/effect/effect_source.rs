//! [`EffectSource`]: what [`Ui::load_effect`](crate::Ui::load_effect) loads under a name.

use std::borrow::Cow;
use std::fmt;

use crate::effect::effect_kind::{ColorEffect, EffectKind};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};

/// An effect's WGSL, the image slots its shapes bind, and what paints while power is reduced.
/// Consumed by [`Ui::load_effect`](crate::Ui::load_effect).
///
/// The WGSL declares `fn effect(input: EffectInput, params: Params) -> EffectOutput`, where
/// `Params` is the struct `U` describes, or `fn effect(input: EffectInput) -> EffectOutput` for an
/// effect with no parameters (`U` of no bytes, as `()`). `effect` is a plain function, with no
/// stage attribute. Palantir declares `EffectInput` and `EffectOutput` after the app's text, so a
/// parse error's position reads in the app's own text, and builds the real fragment stage around
/// `effect`, as SwiftUI's stitchable functions and Skia's SkSL `main` are wrapped.
/// Plain WGSL: an app that wants modules runs WESL or naga_oil itself, as both emit WGSL.
#[must_use]
pub struct EffectSource<U: EffectParams, K: EffectKind = ColorEffect> {
    pub(crate) wgsl: Cow<'static, str>,
    pub(crate) image_slots: u8,
    pub(crate) power_saver: PowerSaverPaint<U, K>,
}

impl<U: EffectParams, K: EffectKind> EffectSource<U, K> {
    /// The most image slots an effect binds, as GL and older Metal tiers lack a bindless array.
    pub const MAX_IMAGE_SLOTS: u8 = MAX_IMAGE_SLOTS;
    /// The most bytes `U` holds, so one storage buffer row serves any effect's instance.
    pub const MAX_PARAMS_SIZE: usize = MAX_PARAMS_SIZE;

    /// An effect of `wgsl` with no image slots, which paints the same while power is reduced.
    pub fn new(wgsl: impl Into<Cow<'static, str>>) -> Self {
        Self {
            wgsl: wgsl.into(),
            image_slots: 0,
            power_saver: PowerSaverPaint::Same,
        }
    }

    /// How many images each shape of the effect binds, at most [`Self::MAX_IMAGE_SLOTS`]; a load
    /// past it returns [`EffectError::TooManyImageSlots`](crate::EffectError::TooManyImageSlots).
    pub const fn image_slots(mut self, slots: u8) -> Self {
        self.image_slots = slots;
        self
    }

    /// What paints in place of the effect while power is reduced
    /// ([`Ui::is_power_reduced`](crate::Ui::is_power_reduced)); [`PowerSaverPaint::Same`] by
    /// default.
    pub const fn power_saver(mut self, paint: PowerSaverPaint<U, K>) -> Self {
        self.power_saver = paint;
        self
    }
}

/// What paints in place of an effect while power is reduced, carried by its source, as a
/// [`BackdropFilter`](crate::BackdropFilter) carries its own fallback. Refused: a theme slot for
/// each power mode, which doubles every slot; a branch at each call site.
pub enum PowerSaverPaint<U: EffectParams, K: EffectKind = ColorEffect> {
    /// The effect itself, as cheap enough already.
    Same,
    /// Another effect with the same parameters and kind, loaded by the time it paints.
    Effect(EffectName<U, K>),
    /// Nothing: the shape paints nothing.
    Nothing,
}

impl<U: EffectParams, K: EffectKind> Clone for PowerSaverPaint<U, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U: EffectParams, K: EffectKind> Copy for PowerSaverPaint<U, K> {}

impl<U: EffectParams, K: EffectKind> PartialEq for PowerSaverPaint<U, K> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Same, Self::Same) | (Self::Nothing, Self::Nothing) => true,
            (Self::Effect(a), Self::Effect(b)) => a == b,
            _ => false,
        }
    }
}

impl<U: EffectParams, K: EffectKind> Eq for PowerSaverPaint<U, K> {}

impl<U: EffectParams, K: EffectKind> fmt::Debug for PowerSaverPaint<U, K> {
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
            .field("power_saver", &self.power_saver)
            .finish()
    }
}
