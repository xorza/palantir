//! Derive for `palantir::widget::Animatable`: animated fields forward to their own impls;
//! `#[animate(snap)]` and `#[animate(lerp_only)]` fields take the target at once in motion and are
//! left out of its arithmetic, a snap field switching halfway through a `lerp` and a `lerp_only`
//! field blending there.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use std::fmt;
use syn::{
    Data, DataStruct, DeriveInput, Expr, ExprPath, Field, Fields, Ident, Type, parse_macro_input,
};

/// `#[derive(Animatable)]` on a struct with named fields. `lerp` returns either end exactly at
/// `t` of 0 or 1. `to_motion` and `from_motion` forward to each animated field, so a struct of
/// colors springs in premultiplied Oklab as its colors do.
///
/// Field attributes, one at most:
/// - `#[animate(snap)]`, a value with no blend: `lerp` takes `a`'s below `t = 0.5` and `b`'s
///   from it, as Flutter's `ThemeData.lerp` and CSS's discrete animation type switch; motion
///   takes the target at once, as its arithmetic keeps `self`, and the magnitude excludes it.
///   `zero` is its `Default`.
/// - `#[animate(lerp_only)]`, a value that blends in `lerp` through its own impl but takes the
///   target at once in motion, as a snap field does: one whose every step costs work a fade
///   should not pay, as a font size reshapes its text. `zero` is its own.
///
/// Struct attributes:
/// - `#[animate(settle_eps = EXPR)]` sets one settle tolerance in the value's own unit
///   (`settle_distance_squared` = `magnitude_squared / EXPR²`); without it, the animated fields'
///   own sum.
/// - `#[animate(before_lerp = PATH)]` names a `fn(&mut Self, &mut Self)` that `lerp` runs on its
///   two ends before it blends them, past the exact ends: a value whose fields blend against
///   context another field holds, as a theme's text overrides fold onto its ambient style.
#[proc_macro_derive(Animatable, attributes(animate))]
pub fn derive_animatable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Animatable can only be derived on structs with named fields",
            ));
        }
    };

    let ContainerOptions {
        settle_eps,
        before_lerp,
    } = ContainerOptions::of(input)?;

    let mut animation: Vec<(&Ident, &Type)> = Vec::new();
    let mut lerp_only: Vec<(&Ident, &Type)> = Vec::new();
    let mut snap: Vec<(&Ident, &Type)> = Vec::new();
    for f in fields {
        let ident = f.ident.as_ref().expect("named fields have idents");
        match FieldKind::of(f)? {
            FieldKind::Animated => animation.push((ident, &f.ty)),
            FieldKind::LerpOnly => lerp_only.push((ident, &f.ty)),
            FieldKind::Snap => snap.push((ident, &f.ty)),
        }
    }

    let lerp_blend = animation.iter().chain(&lerp_only).map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::lerp(a.#f, b.#f, t), }
    });
    let lerp_snap = snap.iter().map(|(f, _)| {
        quote! { #f: if t < 0.5 { a.#f } else { b.#f }, }
    });
    let before_lerp_call = before_lerp.map(|path| {
        quote! {
            let (mut a, mut b) = (a, b);
            #path(&mut a, &mut b);
        }
    });
    // A lerp-only field keeps `self` in motion, as a snap field does.
    let keep_snap: Vec<TokenStream2> = lerp_only
        .iter()
        .chain(&snap)
        .map(|(f, _)| quote! { #f: self.#f, })
        .collect();

    let sub_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::sub(self.#f, other.#f), }
    });

    let add_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::add(self.#f, other.#f), }
    });

    let scale_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::scale(self.#f, k), }
    });

    let magnitude_squared_body = sum_or_zero(
        animation
            .iter()
            .map(|(f, _)| quote! { ::palantir::widget::Animatable::magnitude_squared(self.#f) }),
    );

    let settle_distance_squared_body = if let Some(eps) = &settle_eps {
        quote! {
            let eps: f32 = #eps;
            ::palantir::widget::Animatable::magnitude_squared(self) / (eps * eps)
        }
    } else {
        sum_or_zero(animation.iter().map(|(f, _)| {
            quote! { ::palantir::widget::Animatable::settle_distance_squared(self.#f) }
        }))
    };

    let to_motion_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::to_motion(self.#f), }
    });
    let from_motion_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::from_motion(motion.#f), }
    });
    let motion_snap = lerp_only
        .iter()
        .chain(&snap)
        .map(|(f, _)| quote! { #f: motion.#f, });

    let zero_animation = animation.iter().chain(&lerp_only).map(|(f, ty)| {
        quote! { #f: <#ty as ::palantir::widget::Animatable>::zero(), }
    });
    let zero_snap = snap.iter().map(|(f, ty)| {
        quote! { #f: <#ty as ::core::default::Default>::default(), }
    });
    let normalize_for_spring_animation = animation.iter().map(|(f, ty)| {
        quote! {
            <#ty as ::palantir::widget::Animatable>::normalize_for_spring(
                &mut self.#f,
                &target.#f,
                &mut velocity.#f,
            );
        }
    });

    // `#[inline]` on each method so cross-crate callers (palantir's `tick` into user impls) can inline.
    let expanded = quote! {
        impl #impl_generics ::palantir::widget::Animatable for #name #ty_generics #where_clause {
            #[inline]
            fn lerp(a: Self, b: Self, t: f32) -> Self {
                if t == 0.0 {
                    return a;
                }
                if t == 1.0 {
                    return b;
                }
                #before_lerp_call
                Self {
                    #(#lerp_blend)*
                    #(#lerp_snap)*
                }
            }
            #[inline]
            fn sub(self, other: Self) -> Self {
                Self {
                    #(#sub_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn add(self, other: Self) -> Self {
                Self {
                    #(#add_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn scale(self, k: f32) -> Self {
                Self {
                    #(#scale_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn magnitude_squared(self) -> f32 {
                #magnitude_squared_body
            }
            #[inline]
            fn settle_distance_squared(self) -> f32 {
                #settle_distance_squared_body
            }
            #[inline]
            fn zero() -> Self {
                Self {
                    #(#zero_animation)*
                    #(#zero_snap)*
                }
            }
            #[inline]
            fn to_motion(self) -> Self {
                Self {
                    #(#to_motion_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn from_motion(motion: Self) -> Self {
                Self {
                    #(#from_motion_animation)*
                    #(#motion_snap)*
                }
            }
            #[inline]
            fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
                let _ = (&*self, target, &*velocity);
                #(#normalize_for_spring_animation)*
            }
        }
    };

    Ok(expanded)
}

/// `a + b + …` over `terms`, or `0.0_f32` for none.
fn sum_or_zero(terms: impl IntoIterator<Item = TokenStream2>) -> TokenStream2 {
    let terms: Vec<TokenStream2> = terms.into_iter().collect();
    if terms.is_empty() {
        quote! { 0.0_f32 }
    } else {
        quote! { #(#terms)+* }
    }
}

/// The struct's `#[animate(...)]` options.
#[derive(Default)]
struct ContainerOptions {
    settle_eps: Option<Expr>,
    before_lerp: Option<ExprPath>,
}

/// By the options' tokens, as syn's `Debug` sits behind a feature this crate leaves off.
impl fmt::Debug for ContainerOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn tokens(option: Option<&impl ToTokens>) -> Option<String> {
            option.map(|t| t.to_token_stream().to_string())
        }
        f.debug_struct("ContainerOptions")
            .field("settle_eps", &tokens(self.settle_eps.as_ref()))
            .field("before_lerp", &tokens(self.before_lerp.as_ref()))
            .finish()
    }
}

impl ContainerOptions {
    /// Unknown options and an option given twice error.
    fn of(input: &DeriveInput) -> syn::Result<Self> {
        let mut options = Self::default();
        for attr in &input.attrs {
            if !attr.path().is_ident("animate") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("settle_eps") {
                    if options.settle_eps.is_some() {
                        return Err(meta.error("`settle_eps` given twice"));
                    }
                    options.settle_eps = Some(meta.value()?.parse::<Expr>()?);
                    Ok(())
                } else if meta.path.is_ident("before_lerp") {
                    if options.before_lerp.is_some() {
                        return Err(meta.error("`before_lerp` given twice"));
                    }
                    options.before_lerp = Some(meta.value()?.parse::<ExprPath>()?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown #[animate(...)] option on a struct; expected `settle_eps` or `before_lerp`",
                    ))
                }
            })?;
        }
        Ok(options)
    }
}

/// How the derive treats one field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldKind {
    /// Forwarded to the field's own `Animatable` impl.
    Animated,
    /// `#[animate(lerp_only)]`: blends in `lerp`, takes the target at once in motion.
    LerpOnly,
    /// `#[animate(snap)]`: switches halfway through `lerp`, takes the target at once in motion.
    Snap,
}

impl FieldKind {
    /// Unknown idents and a second option error, so typos fail to compile.
    fn of(f: &Field) -> syn::Result<Self> {
        let mut kind = Self::Animated;
        for attr in &f.attrs {
            if !attr.path().is_ident("animate") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                let next = if meta.path.is_ident("snap") {
                    Self::Snap
                } else if meta.path.is_ident("lerp_only") {
                    Self::LerpOnly
                } else {
                    return Err(meta
                        .error("unknown #[animate(...)] option; expected `snap` or `lerp_only`"));
                };
                if kind != Self::Animated {
                    return Err(meta.error("a field takes one of `snap` and `lerp_only`"));
                }
                kind = next;
                Ok(())
            })?;
        }
        Ok(kind)
    }
}

#[cfg(test)]
mod tests {
    use crate::expand;
    use syn::{DeriveInput, parse_quote};

    /// Refused inputs and their messages.
    #[test]
    fn refused_inputs_name_the_reason() {
        let shape = "Animatable can only be derived on structs with named fields";
        let option = "unknown #[animate(...)] option; expected `snap` or `lerp_only`";
        let container =
            "unknown #[animate(...)] option on a struct; expected `settle_eps` or `before_lerp`";
        let cases: [(&str, DeriveInput, &str); 10] = [
            ("enum", parse_quote! { enum E { A } }, shape),
            ("tuple struct", parse_quote! { struct T(f32); }, shape),
            ("unit struct", parse_quote! { struct U; }, shape),
            (
                "typo",
                parse_quote! { struct S { #[animate(snip)] a: f32 } },
                option,
            ),
            (
                "skip, which is not a spelling of snap",
                parse_quote! { struct S { #[animate(skip)] a: f32 } },
                option,
            ),
            (
                "field option on the struct",
                parse_quote! { #[animate(snap)] struct S { a: f32 } },
                container,
            ),
            (
                "two tolerances",
                parse_quote! { #[animate(settle_eps = 0.5, settle_eps = 0.25)] struct S { a: f32 } },
                "`settle_eps` given twice",
            ),
            (
                "two hooks",
                parse_quote! { #[animate(before_lerp = f, before_lerp = g)] struct S { a: f32 } },
                "`before_lerp` given twice",
            ),
            (
                "two field options in one attribute",
                parse_quote! { struct S { #[animate(snap, lerp_only)] a: f32 } },
                "a field takes one of `snap` and `lerp_only`",
            ),
            (
                "two field options in two attributes",
                parse_quote! { struct S { #[animate(lerp_only)] #[animate(snap)] a: f32 } },
                "a field takes one of `snap` and `lerp_only`",
            ),
        ];
        for (label, input, message) in cases {
            let error = expand(&input).expect_err(label);
            assert_eq!(error.to_string(), message, "{label}");
        }
    }
}
