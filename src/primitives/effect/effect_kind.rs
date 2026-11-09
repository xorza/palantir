//! [`EffectKind`]: what an effect reads besides its own inputs, a type each [`EffectName`] names.
//!
//! [`EffectName`]: crate::EffectName

use std::any::{self, TypeId};
use std::fmt;

/// The kind of an effect: [`ColorEffect`] or [`BackdropEffect`], each with its own WGSL
/// signature. Sealed, as palantir builds each kind's pipeline.
pub trait EffectKind: sealed::Kind {}

/// An effect that reads its position, size, corners, time, parameters and images, as SwiftUI's
/// `colorEffect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorEffect;

/// An effect that also reads the pixels painted below it, through the capture
/// [`BackdropFilter`](crate::BackdropFilter) uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackdropEffect;

impl EffectKind for ColorEffect {}
impl EffectKind for BackdropEffect {}

/// Which kind an effect is, at run time: what its contract declares and its pipeline binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KindId {
    Color,
    Backdrop,
}

mod sealed {
    #[expect(
        unnameable_types,
        private_interfaces,
        reason = "the seal: nothing outside the crate can name it, so no other kind can exist and \
                  the crate-private id cannot leak"
    )]
    pub trait Kind: 'static {
        const ID: KindId;
    }

    use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, KindId};

    #[expect(
        private_interfaces,
        reason = "the sealed trait's id, which nothing outside the crate can name"
    )]
    impl Kind for ColorEffect {
        const ID: KindId = KindId::Color;
    }
    #[expect(
        private_interfaces,
        reason = "the sealed trait's id, which nothing outside the crate can name"
    )]
    impl Kind for BackdropEffect {
        const ID: KindId = KindId::Backdrop;
    }
}

/// The run-time identity of an effect's parameter type and kind, which a name's types must match
/// wherever it is used again; the type names are for the panic that says they do not.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectTypes {
    params: TypeId,
    kind: TypeId,
    params_name: &'static str,
    kind_name: &'static str,
}

impl EffectTypes {
    pub(crate) fn of<U: 'static, K: EffectKind>() -> Self {
        Self {
            params: TypeId::of::<U>(),
            kind: TypeId::of::<K>(),
            params_name: any::type_name::<U>(),
            kind_name: any::type_name::<K>(),
        }
    }
}

impl PartialEq for EffectTypes {
    fn eq(&self, other: &Self) -> bool {
        self.params == other.params && self.kind == other.kind
    }
}

impl Eq for EffectTypes {}

impl fmt::Display for EffectTypes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.params_name, self.kind_name)
    }
}
