//! [`EffectTypes`]: an effect's parameter type and kind at run time.

use std::any::{self, TypeId};
use std::fmt;

use crate::primitives::effect::effect_kind::EffectKind;

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
