//! The effect contract: the declarations that follow the app's WGSL, palantir's stages around it,
//! and the checks an effect passes before it loads.

use std::fmt::Write as _;

use naga::front::wgsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{Module, ScalarKind, StructMember, TypeInner, VectorSize};

use crate::effect::effect_kind::KindId;
use crate::effect::effect_params::{EffectParam, ParamType};
use crate::effect::effect_store::EffectModule;
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};
use crate::primitives::paint::antialias::AA_HALF_WIDTH;

/// The public contract, `EffectInput`, `EffectOutput` and the blends, which follows the app's
/// text, as WGSL declarations hold in any order, so a position in a parse error reads in the app's
/// text.
const CONTRACT: &str = include_str!("contract.wgsl");

/// What a backdrop effect reads besides: `effect_backdrop` over the pane's result, which follows
/// the contract for that kind alone.
const BACKDROP: &str = include_str!("backdrop.wgsl");

/// The bindings the backdrop contract declares, which an app's text names not.
const BACKDROP_BINDINGS: [&str; 3] = [
    "palantir_results",
    "palantir_results_sampler",
    "palantir_panes",
];

/// What palantir's fragment stage sets before it calls a backdrop effect: the pane it draws, its
/// rect's top-left and the scale `effect_backdrop` maps a logical position by.
const BACKDROP_SET: &str =
    "palantir_pane = v.pane;\n    palantir_origin = v.rect.xy;\n    palantir_scale = scale;\n    ";
/// Palantir's own stages around `effect`, with markers for the antialiasing reach, the parameter
/// read and the call.
const WRAPPER: &str = include_str!("wrapper.wgsl");

/// What the module starts with: the enable for the wrapper's dual-source output, on the app's first
/// line, so the app's line numbers hold.
const ENABLE: &str = "enable dual_source_blending; ";

const ENTRY: &str = "effect";
const INPUT: &str = "EffectInput";
const OUTPUT: &str = "EffectOutput";

/// What an effect's parameter struct must match: `U`'s fields and size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParamsLayout {
    pub(crate) fields: &'static [EffectParam],
    pub(crate) size: usize,
}

/// The group an effect's images bind at, past the parameter words at group 0.
pub(crate) const IMAGES_GROUP: u32 = 1;

/// The group a backdrop effect reads its pane's result through, past its images.
pub(crate) const PANES_GROUP: u32 = 2;

/// The complete module palantir compiles for `wgsl`, an effect of `kind`: the app's text, the
/// contract with its kind's reads and its `image_slots` images, and palantir's stages, once the
/// app's text parses and validates with the contract, declares no binding of its own, its
/// `effect` takes the contract's arguments, and its parameter struct matches `params`.
pub(crate) fn check(
    wgsl: &str,
    kind: KindId,
    params: ParamsLayout,
    image_slots: u8,
) -> Result<EffectModule, EffectError> {
    if image_slots > MAX_IMAGE_SLOTS {
        return Err(EffectError::TooManyImageSlots { slots: image_slots });
    }
    if params.size > MAX_PARAMS_SIZE {
        return Err(EffectError::ParamsTooLarge { size: params.size });
    }
    let reads = match kind {
        KindId::Color => "",
        KindId::Backdrop => BACKDROP,
    };
    let contract = format!("{CONTRACT}{reads}{}", images(image_slots));
    let app = parse_and_validate(format!("{wgsl}\n{contract}"), Capabilities::default())?;
    check_bindings(&app.module, kind, image_slots)?;
    let params_struct = check_entry(&app.module, params)?;
    let full = format!(
        "{ENABLE}{wgsl}\n{contract}{}",
        wrapper(kind, params_struct.as_deref(), params.fields)
    );
    let full = parse_and_validate(
        full,
        Capabilities::default() | Capabilities::DUAL_SOURCE_BLENDING | Capabilities::IMMEDIATES,
    )?;
    Ok(EffectModule {
        text: full.text,
        kind,
        image_slots,
    })
}

/// The contract's image declarations for `slots` images: the sampler at binding 0 of
/// [`IMAGES_GROUP`], then `effect_image0` and on, each at the binding after.
fn images(slots: u8) -> String {
    if slots == 0 {
        return String::new();
    }
    let mut out = format!(
        "\n// The sampler every image reads through: linear, clamped to the edge.\n\
         @group({IMAGES_GROUP}) @binding(0) var effect_sampler: sampler;\n\
         // The shape's images, in the order it binds them; a sample is linear, premultiplied.\n"
    );
    for slot in 0..slots {
        writeln!(
            out,
            "@group({IMAGES_GROUP}) @binding({}) var effect_image{slot}: texture_2d<f32>;",
            slot + 1
        )
        .expect("a `String` takes every write");
    }
    out
}

/// Refuse a binding the app's text declares: palantir binds every resource an effect reads, its
/// images through the contract's `slots`, so a binding of the app's own matches no layout. The
/// contract's own are known by name, which no app declares again, as WGSL refuses a second
/// declaration of one name.
fn check_bindings(module: &Module, kind: KindId, slots: u8) -> Result<(), EffectError> {
    let contract = |name: &str| {
        slots > 0 && name == "effect_sampler"
            || (0..slots).any(|slot| name == format!("effect_image{slot}"))
            || kind == KindId::Backdrop && BACKDROP_BINDINGS.contains(&name)
    };
    let own = module.global_variables.iter().find(|(_, global)| {
        global.binding.is_some() && !global.name.as_deref().is_some_and(contract)
    });
    match own {
        Some((_, global)) => Err(EffectError::Binding {
            name: global.name.clone().unwrap_or_default(),
        }),
        None => Ok(()),
    }
}

/// A text naga parsed and validated, with its module.
#[derive(Debug)]
struct Parsed {
    module: Module,
    text: String,
}

/// `text` parsed and validated under `capabilities`; an error carries the text.
fn parse_and_validate(text: String, capabilities: Capabilities) -> Result<Parsed, EffectError> {
    let module = match wgsl::parse_str(&text) {
        Ok(module) => module,
        Err(source) => {
            return Err(EffectError::Parse {
                source: Box::new(source),
                text,
            });
        }
    };
    if let Err(source) = Validator::new(ValidationFlags::all(), capabilities).validate(&module) {
        return Err(EffectError::Validation {
            source: Box::new(source),
            text,
        });
    }
    Ok(Parsed { module, text })
}

/// Palantir's stages for an effect of `kind`, reading the parameters into `params_struct` from
/// `fields`' words, or with no parameters where `params_struct` is `None`.
fn wrapper(kind: KindId, params_struct: Option<&str>, fields: &[EffectParam]) -> String {
    let (read, call) = match params_struct {
        Some(name) => {
            let members: Vec<String> = fields.iter().map(read_field).collect();
            (
                format!(
                    "\nfn palantir_params(at: u32) -> {name} {{\n    return {name}({});\n}}\n",
                    members.join(", ")
                ),
                "effect(input, palantir_params(v.params))",
            )
        }
        None => (String::new(), "effect(input)"),
    };
    WRAPPER
        .replace("/*{AA_HALF_WIDTH}*/", &format!("{AA_HALF_WIDTH:?}"))
        .replace("/*{PARAMS}*/", &read)
        .replace(
            "/*{BACKDROP_SET}*/",
            match kind {
                KindId::Color => "",
                KindId::Backdrop => BACKDROP_SET,
            },
        )
        .replace("/*{CALL}*/", call)
}

/// The WGSL that reads `field` from the instance's words, starting at `at`.
fn read_field(field: &EffectParam) -> String {
    let word = field.offset / 4;
    let words = |count: usize| -> String {
        (word..word + count)
            .map(|w| format!("palantir_words[at + {w}u]"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match field.ty {
        ParamType::F32 => format!("bitcast<f32>({})", words(1)),
        ParamType::I32 => format!("bitcast<i32>({})", words(1)),
        ParamType::U32 => words(1),
        ParamType::Vec2F32 => format!("bitcast<vec2<f32>>(vec2<u32>({}))", words(2)),
        ParamType::Vec3F32 => format!("bitcast<vec3<f32>>(vec3<u32>({}))", words(3)),
        ParamType::Vec4F32 => format!("bitcast<vec4<f32>>(vec4<u32>({}))", words(4)),
        ParamType::Vec2I32 => format!("bitcast<vec2<i32>>(vec2<u32>({}))", words(2)),
        ParamType::Vec3I32 => format!("bitcast<vec3<i32>>(vec3<u32>({}))", words(3)),
        ParamType::Vec4I32 => format!("bitcast<vec4<i32>>(vec4<u32>({}))", words(4)),
        ParamType::Vec2U32 => format!("vec2<u32>({})", words(2)),
        ParamType::Vec3U32 => format!("vec3<u32>({})", words(3)),
        ParamType::Vec4U32 => format!("vec4<u32>({})", words(4)),
    }
}

/// `effect` against the contract; the name of its parameter struct, where it takes one.
fn check_entry(module: &Module, params: ParamsLayout) -> Result<Option<String>, EffectError> {
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
    let Some(argument) = entry.arguments.get(1) else {
        return Ok(None);
    };
    let ty = &module.types[argument.ty];
    let TypeInner::Struct { members, span } = &ty.inner else {
        return Err(EffectError::Entry(EntryMismatch::Params));
    };
    check_params(module, members, *span, params).map_err(EffectError::ParamLayout)?;
    Ok(Some(ty.name.clone().expect("a WGSL struct has a name")))
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
