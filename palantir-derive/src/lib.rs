//! Palantir's derive macros. For `palantir::widget::Lerp` and `palantir::widget::Animatable`:
//! `Lerp` blends a struct fieldwise; `Animatable` adds the spring arithmetic, where animated
//! fields forward to their own impls and `#[animate(snap)]` and `#[animate(lerp_only)]` fields
//! take the target at once and are left out of it. A snap field switches halfway through a
//! `lerp`; a `lerp_only` field blends there. A hidden `ThemeWalk` derive is palantir's own, for
//! its theme parts' walks over text and press slots.

mod animatable;
mod effect_params;
mod lerp;
mod options;
mod theme_walk;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// `#[derive(Lerp)]` on a struct with named fields: `lerp` returns either end exactly at `t` of 0
/// or 1, and blends each field through its own `Lerp` between them. `lerp_into` does the same in
/// place, each field through its own `lerp_into`, so a nested value copies none of its ends.
///
/// Field attributes:
/// - `#[animate(snap)]`, a value with no blend: `lerp` takes `a`'s below `t = 0.5` and `b`'s
///   from it, as Flutter's `ThemeData.lerp` and CSS's discrete animation type switch.
/// - `#[animate(lerp_only)]` blends as an unmarked field does; it matters to `Animatable`.
///
/// Struct attributes:
/// - `#[animate(before_lerp = PATH)]` names a `fn(&mut Self, &mut Self)` that `lerp` runs on its
///   two ends before it blends them, past the exact ends: a value whose fields blend against
///   context another field holds, as a theme's text overrides fold onto its ambient style.
///   `lerp_into` runs it on copies of the ends, and an inherent `lerp_ends_into` blends ends it
///   already ran on, for a caller that blends one pair at many phases.
/// - `#[animate(coerce = PATH)]` names a `fn(&mut Self)` that `lerp` and `lerp_into` run on the
///   blend, past the exact ends: a value with a floor, as a width a curve overshooting below zero
///   would leave negative. `Animatable` runs it on `from_motion` too.
#[proc_macro_derive(Lerp, attributes(animate))]
pub fn derive_lerp(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    lerp::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(Animatable)]` on a struct with named fields that also implements `Lerp`, as
/// `#[derive(Lerp)]` gives it. `to_motion` and `from_motion` forward to each animated field, so a
/// struct of colors springs in premultiplied Oklab as its colors do.
///
/// Field attributes, one at most of `snap` and `lerp_only`:
/// - `#[animate(snap)]` takes the target at once in motion, as its arithmetic keeps `self`, and
///   the magnitude excludes it. `zero` is its `Default`.
/// - `#[animate(lerp_only)]`, a value that blends in `lerp` but takes the target at once in
///   motion, as a snap field does: one whose every step costs work a fade should not pay, as a
///   font size reshapes its text. `zero` is its own.
/// - `#[animate(settle_eps = EXPR)]` on an animated field: its settle tolerance in its own unit,
///   where its type's is unit-free, as a width in pixels held in an `f32`.
///
/// Struct attributes:
/// - `#[animate(settle_eps = EXPR)]` sets one settle tolerance in the value's own unit
///   (`settle_distance_squared` = `magnitude_squared / EXPR²`); without it, the animated fields'
///   own sum.
/// - `#[animate(coerce = PATH)]`, as `Lerp` reads it: `from_motion` runs it on the value it
///   returns, so a spring or an eased motion that overshoots lands in the value's domain.
#[proc_macro_derive(Animatable, attributes(animate))]
pub fn derive_animatable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    animatable::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(ThemeWalk)]`, palantir's own, on a theme part: its walks over text, press and
/// material slots visit every field in field order, each through the field type's own
/// `ThemeWalk`, so a field whose type has no walk fails to compile until it gets one, as serde's
/// and `gc`'s derives ask of each field.
///
/// Hidden: its expansion names palantir's crate-private `crate::theme::ThemeWalk`, so it compiles
/// inside palantir alone, and a proc-macro crate can export nothing but its macros. Refused: a
/// second derive crate for one private derive.
#[doc(hidden)]
#[proc_macro_derive(ThemeWalk)]
pub fn derive_theme_walk(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    theme_walk::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(EffectParams)]` on a `#[repr(C)]` struct with named fields and no generics: each
/// field's name, byte offset and WGSL type, which `palantir::Ui::load_effect` checks against the
/// effect's parameter struct, member for member. Each field's type must be a
/// `palantir::ParamValue`. The struct also derives `bytemuck::Pod`, which the trait requires.
#[proc_macro_derive(EffectParams)]
pub fn derive_effect_params(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    effect_params::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
