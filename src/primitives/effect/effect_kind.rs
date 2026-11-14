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
/// [`Material`](crate::Material) uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackdropEffect;

impl EffectKind for ColorEffect {}
impl EffectKind for BackdropEffect {}

pub(crate) use sealed::{BackdropRead, ColorRead};

/// A paint's read with its kind erased, as a shape's draw holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ErasedRead<'a> {
    Color(ColorRead),
    Backdrop(&'a BackdropRead),
}

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
        /// What a paint of the kind reads besides its inputs: a color effect's halo, a backdrop
        /// effect's blur and fallback.
        type Read: Clone + fmt::Debug;

        /// What a paint of the kind starts with: over its shape; read sharp, with the theme's
        /// surface for a fallback.
        const READ: Self::Read;

        /// `read` with its kind erased.
        fn erase(read: &Self::Read) -> ErasedRead<'_>;
    }

    use std::fmt;

    use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, ErasedRead, KindId};
    use crate::primitives::paint::material::backdrop_fallback::BackdropFallback;

    /// What a color effect's paint reads besides its inputs: a halo `outside` its shape alone,
    /// logical px past it; `None` paints over the shape. Public in the seal, so nothing outside
    /// the crate can name it.
    #[derive(Clone, Copy, Debug)]
    #[expect(unnameable_types, reason = "a kind's read, behind the seal")]
    pub struct ColorRead {
        pub(crate) outside: Option<f32>,
    }

    /// What a backdrop effect's paint reads: what is painted under it, blurred by σ `blur`
    /// logical px, and what it paints in its place while transparency is reduced.
    #[derive(Clone, Debug)]
    #[expect(unnameable_types, reason = "a kind's read, behind the seal")]
    pub struct BackdropRead {
        pub(crate) blur: f32,
        pub(crate) fallback: BackdropFallback,
    }

    #[expect(
        private_interfaces,
        reason = "the sealed trait's id, which nothing outside the crate can name"
    )]
    impl Kind for ColorEffect {
        const ID: KindId = KindId::Color;
        type Read = ColorRead;

        const READ: ColorRead = ColorRead { outside: None };

        fn erase(read: &ColorRead) -> ErasedRead<'_> {
            ErasedRead::Color(*read)
        }
    }
    #[expect(
        private_interfaces,
        reason = "the sealed trait's id, which nothing outside the crate can name"
    )]
    impl Kind for BackdropEffect {
        const ID: KindId = KindId::Backdrop;
        type Read = BackdropRead;

        const READ: BackdropRead = BackdropRead {
            blur: 0.0,
            fallback: BackdropFallback::ThemeSurface,
        };

        fn erase(read: &BackdropRead) -> ErasedRead<'_> {
            ErasedRead::Backdrop(read)
        }
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
