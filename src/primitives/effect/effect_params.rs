//! [`EffectParams`]: the parameters an effect's WGSL reads, laid out as its `Params` struct.

use glam::{IVec2, IVec3, IVec4, UVec2, UVec3, UVec4, Vec2, Vec3, Vec4};

use crate::primitives::paint::color::RgbaF32;

/// The parameters of one instance of an effect: a `Pod` struct whose fields match the WGSL
/// struct the effect's `effect` function takes as its second argument, member for member, in
/// name, offset and type, and in size. `#[derive(EffectParams)]` writes [`Self::FIELDS`] from a
/// `#[repr(C)]` struct; [`Ui::load_effect`](crate::Ui::load_effect) checks them against the WGSL,
/// and rustc checks each value at each shape. `()` is an effect with no parameters, whose
/// `effect` takes its input alone. Refused: values by name (Qt, Skia), a lookup and a check per
/// value per frame; a float array by index (Flutter), with no check.
pub trait EffectParams: bytemuck::Pod {
    /// Each field in declaration order, with its byte offset and its WGSL type.
    const FIELDS: &'static [EffectParam];
}

impl EffectParams for () {
    const FIELDS: &'static [EffectParam] = &[];
}

/// One field of an [`EffectParams`] struct, as `#[derive(EffectParams)]` describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectParam {
    /// The field's name, which the WGSL member's must equal.
    pub name: &'static str,
    /// Bytes from the start of the struct.
    pub offset: usize,
    /// The WGSL type the field's Rust type stands for.
    pub ty: ParamType,
}

/// A WGSL type an effect parameter can hold: a 32-bit scalar or a vector of 2 to 4 of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamType {
    /// `f32`.
    F32,
    /// `i32`.
    I32,
    /// `u32`.
    U32,
    /// `vec2<f32>`.
    Vec2F32,
    /// `vec3<f32>`.
    Vec3F32,
    /// `vec4<f32>`.
    Vec4F32,
    /// `vec2<i32>`.
    Vec2I32,
    /// `vec3<i32>`.
    Vec3I32,
    /// `vec4<i32>`.
    Vec4I32,
    /// `vec2<u32>`.
    Vec2U32,
    /// `vec3<u32>`.
    Vec3U32,
    /// `vec4<u32>`.
    Vec4U32,
}

impl ParamType {
    /// The type as WGSL spells it.
    pub const fn wgsl(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::Vec2F32 => "vec2<f32>",
            Self::Vec3F32 => "vec3<f32>",
            Self::Vec4F32 => "vec4<f32>",
            Self::Vec2I32 => "vec2<i32>",
            Self::Vec3I32 => "vec3<i32>",
            Self::Vec4I32 => "vec4<i32>",
            Self::Vec2U32 => "vec2<u32>",
            Self::Vec3U32 => "vec3<u32>",
            Self::Vec4U32 => "vec4<u32>",
        }
    }
}

/// A Rust type an [`EffectParams`] field can be, with the WGSL type it stands for.
/// `#[derive(EffectParams)]` reads it from each field's type.
pub trait ParamValue {
    /// The WGSL type this Rust type stands for.
    const TYPE: ParamType;
}

macro_rules! param_values {
    ($($ty:ty => $param:ident),* $(,)?) => {
        $(impl ParamValue for $ty {
            const TYPE: ParamType = ParamType::$param;
        })*
    };
}

param_values! {
    f32 => F32,
    i32 => I32,
    u32 => U32,
    [f32; 2] => Vec2F32,
    [f32; 3] => Vec3F32,
    [f32; 4] => Vec4F32,
    [i32; 2] => Vec2I32,
    [i32; 3] => Vec3I32,
    [i32; 4] => Vec4I32,
    [u32; 2] => Vec2U32,
    [u32; 3] => Vec3U32,
    [u32; 4] => Vec4U32,
    Vec2 => Vec2F32,
    Vec3 => Vec3F32,
    Vec4 => Vec4F32,
    IVec2 => Vec2I32,
    IVec3 => Vec3I32,
    IVec4 => Vec4I32,
    UVec2 => Vec2U32,
    UVec3 => Vec3U32,
    UVec4 => Vec4U32,
    RgbaF32 => Vec4F32,
}
