//! The effect contract: palantir's prelude, which follows the app's WGSL, and the checks an effect
//! passes before it loads.

use naga::front::wgsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{Module, ScalarKind, StructMember, TypeInner, VectorSize};

use crate::effect::effect_params::{EffectParam, ParamType};
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};

/// What palantir declares for every effect. It follows the app's text, as WGSL declarations hold
/// in any order, so a position in a parse error reads in the app's text.
const PRELUDE: &str = "
struct EffectInput {
    pos: vec2<f32>,
    size: vec2<f32>,
    corners: vec4<f32>,
    time: f32,
}

struct EffectOutput {
    color: vec4<f32>,
    under: vec4<f32>,
}
";

const ENTRY: &str = "effect";
const INPUT: &str = "EffectInput";
const OUTPUT: &str = "EffectOutput";

/// What an effect's parameter struct must match: `U`'s fields and size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParamsLayout {
    pub(crate) fields: &'static [EffectParam],
    pub(crate) size: usize,
}

/// `wgsl` with the prelude, parsed and validated, once its `effect` takes the contract's
/// arguments and its parameter struct matches `params`.
pub(crate) fn check(
    wgsl: &str,
    params: ParamsLayout,
    image_slots: u8,
) -> Result<Module, EffectError> {
    if image_slots > MAX_IMAGE_SLOTS {
        return Err(EffectError::TooManyImageSlots { slots: image_slots });
    }
    if params.size > MAX_PARAMS_SIZE {
        return Err(EffectError::ParamsTooLarge { size: params.size });
    }
    let text = format!("{wgsl}\n{PRELUDE}");
    let module = match wgsl::parse_str(&text) {
        Ok(module) => module,
        Err(source) => {
            return Err(EffectError::Parse {
                source: Box::new(source),
                text,
            });
        }
    };
    if let Err(source) =
        Validator::new(ValidationFlags::all(), Capabilities::default()).validate(&module)
    {
        return Err(EffectError::Validation {
            source: Box::new(source),
            text,
        });
    }
    check_entry(&module, params)?;
    Ok(module)
}

fn check_entry(module: &Module, params: ParamsLayout) -> Result<(), EffectError> {
    if module.entry_points.iter().any(|entry| entry.name == ENTRY) {
        return Err(EffectError::Entry(EntryMismatch::Stage));
    }
    let (_, entry) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some(ENTRY))
        .ok_or(EffectError::MissingEntry)?;
    let named = |name: &str| {
        module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some(name))
            .map(|(handle, _)| handle)
    };
    // By size, not by field count: a struct of no bytes is no struct to the WGSL, and one that
    // lists no fields but holds bytes is checked as a struct, which its WGSL members contradict.
    let want = if params.size == 0 { 1 } else { 2 };
    let got = entry.arguments.len();
    if got != want {
        return Err(EffectError::Entry(EntryMismatch::ArgumentCount {
            want,
            got,
        }));
    }
    if Some(entry.arguments[0].ty) != named(INPUT) {
        return Err(EffectError::Entry(EntryMismatch::Input));
    }
    if entry.result.as_ref().map(|result| result.ty) != named(OUTPUT) {
        return Err(EffectError::Entry(EntryMismatch::Output));
    }
    if let Some(argument) = entry.arguments.get(1) {
        let TypeInner::Struct { members, span } = &module.types[argument.ty].inner else {
            return Err(EffectError::Entry(EntryMismatch::Params));
        };
        check_params(module, members, *span, params).map_err(EffectError::ParamLayout)?;
    }
    Ok(())
}

/// The WGSL struct against `U`, member for member: count, then each name, offset and type, then
/// the span, which an array of them strides by.
fn check_params(
    module: &Module,
    members: &[StructMember],
    span: u32,
    params: ParamsLayout,
) -> Result<(), ParamMismatch> {
    if members.len() != params.fields.len() {
        return Err(ParamMismatch::FieldCount {
            wgsl: members.len(),
            rust: params.fields.len(),
        });
    }
    for (idx, (member, field)) in members.iter().zip(params.fields).enumerate() {
        let wgsl_name = member.name.as_deref().unwrap_or_default();
        if wgsl_name != field.name {
            return Err(ParamMismatch::Name {
                idx,
                wgsl: wgsl_name.to_owned(),
                rust: field.name,
            });
        }
        if usize::try_from(member.offset) != Ok(field.offset) {
            return Err(ParamMismatch::Offset {
                name: field.name,
                wgsl: member.offset,
                rust: field.offset,
            });
        }
        if param_type(&module.types[member.ty].inner) != Some(field.ty) {
            return Err(ParamMismatch::Type {
                name: field.name,
                rust: field.ty,
            });
        }
    }
    if usize::try_from(span) != Ok(params.size) {
        return Err(ParamMismatch::Size {
            wgsl: span,
            rust: params.size,
        });
    }
    Ok(())
}

/// The parameter type a WGSL type is, `None` for one no parameter holds.
const fn param_type(inner: &TypeInner) -> Option<ParamType> {
    let (size, scalar) = match *inner {
        TypeInner::Scalar(scalar) => (None, scalar),
        TypeInner::Vector { size, scalar } => (Some(size), scalar),
        _ => return None,
    };
    if scalar.width != 4 {
        return None;
    }
    Some(match (scalar.kind, size) {
        (ScalarKind::Float, None) => ParamType::F32,
        (ScalarKind::Sint, None) => ParamType::I32,
        (ScalarKind::Uint, None) => ParamType::U32,
        (ScalarKind::Float, Some(VectorSize::Bi)) => ParamType::Vec2F32,
        (ScalarKind::Float, Some(VectorSize::Tri)) => ParamType::Vec3F32,
        (ScalarKind::Float, Some(VectorSize::Quad)) => ParamType::Vec4F32,
        (ScalarKind::Sint, Some(VectorSize::Bi)) => ParamType::Vec2I32,
        (ScalarKind::Sint, Some(VectorSize::Tri)) => ParamType::Vec3I32,
        (ScalarKind::Sint, Some(VectorSize::Quad)) => ParamType::Vec4I32,
        (ScalarKind::Uint, Some(VectorSize::Bi)) => ParamType::Vec2U32,
        (ScalarKind::Uint, Some(VectorSize::Tri)) => ParamType::Vec3U32,
        (ScalarKind::Uint, Some(VectorSize::Quad)) => ParamType::Vec4U32,
        _ => return None,
    })
}
