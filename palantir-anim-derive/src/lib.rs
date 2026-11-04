//! Derives for `palantir::widget::Lerp` and `palantir::widget::Animatable`. `Lerp` blends a struct
//! fieldwise; `Animatable` adds the spring arithmetic, where animated fields forward to their own
//! impls and `#[animate(snap)]` and `#[animate(lerp_only)]` fields take the target at once and are
//! left out of it. A snap field switches halfway through a `lerp`; a `lerp_only` field blends
//! there. A hidden `TextSlots` derive is palantir's own, for its theme parts' text walk.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use std::fmt;
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{
    Data, DataStruct, DeriveInput, Expr, ExprPath, Field, Fields, Ident, Type, parse_macro_input,
};

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
    expand_lerp(&input)
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
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(TextSlots)]`, palantir's own, on a theme part: its text walk visits every field in
/// field order, each through the field type's own `TextSlots`, so a field whose type has no walk
/// fails to compile until it gets one, as serde's and `gc`'s derives ask of each field.
///
/// Hidden: its expansion names palantir's crate-private `crate::theme::TextSlots`, so it compiles
/// inside palantir alone, and a proc-macro crate can export nothing but its macros. Refused: a
/// second derive crate for one private derive.
#[doc(hidden)]
#[proc_macro_derive(TextSlots)]
pub fn derive_text_slots(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_text_slots(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The named fields of a struct, or the error each derive gives any other input.
fn named_fields<'a>(
    input: &'a DeriveInput,
    derive: &str,
) -> syn::Result<&'a Punctuated<Field, Comma>> {
    match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => Ok(&named.named),
        _ => Err(syn::Error::new_spanned(
            input,
            format!("{derive} can only be derived on structs with named fields"),
        )),
    }
}

fn expand_lerp(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let fields = named_fields(input, "Lerp")?;
    let ContainerOptions {
        before_lerp,
        coerce,
        ..
    } = ContainerOptions::of(input)?;

    let mut blend = Vec::new();
    let mut blend_into = Vec::new();
    for f in fields {
        let ident = f.ident.as_ref().expect("named fields have idents");
        let kind = FieldOptions::of(f)?.kind;
        blend.push(match kind {
            FieldKind::Snap => quote! {
                #ident: ::core::clone::Clone::clone(if t < 0.5 { &a.#ident } else { &b.#ident }),
            },
            FieldKind::Animated | FieldKind::LerpOnly => {
                quote! { #ident: ::palantir::widget::Lerp::lerp(&a.#ident, &b.#ident, t), }
            }
        });
        blend_into.push(match kind {
            FieldKind::Snap => quote! {
                ::core::clone::Clone::clone_from(&mut out.#ident, if t < 0.5 { &a.#ident } else { &b.#ident });
            },
            FieldKind::Animated | FieldKind::LerpOnly => quote! {
                ::palantir::widget::Lerp::lerp_into(&a.#ident, &b.#ident, t, &mut out.#ident);
            },
        });
    }
    let coerce_out = coerce.as_ref().map(|path| quote! { #path(out); });
    let exact_ends_into = quote! {
        if t == 0.0 {
            ::core::clone::Clone::clone_from(out, a);
            return;
        }
        if t == 1.0 {
            ::core::clone::Clone::clone_from(out, b);
            return;
        }
    };
    let (before_lerp_call, lerp_into_body, ends_into) = match before_lerp {
        Some(path) => (
            Some(quote! {
                let (mut a, mut b) = (::core::clone::Clone::clone(a), ::core::clone::Clone::clone(b));
                #path(&mut a, &mut b);
                let (a, b) = (&a, &b);
            }),
            quote! {
                #exact_ends_into
                let (mut a, mut b) = (::core::clone::Clone::clone(a), ::core::clone::Clone::clone(b));
                #path(&mut a, &mut b);
                Self::lerp_ends_into(&a, &b, t, out);
            },
            Some(quote! {
                impl #impl_generics #name #ty_generics #where_clause {
                    /// [`Lerp::lerp_into`](::palantir::widget::Lerp::lerp_into) of ends the
                    /// `before_lerp` hook already ran on, as a caller that blends one pair at many
                    /// phases runs it once; the exact ends are the caller's.
                    #[inline]
                    pub(crate) fn lerp_ends_into(a: &Self, b: &Self, t: f32, out: &mut Self) {
                        #(#blend_into)*
                        #coerce_out
                    }
                }
            }),
        ),
        None => (
            None,
            quote! {
                #exact_ends_into
                #(#blend_into)*
                #coerce_out
            },
            None,
        ),
    };
    let blended = coerced(
        coerce.as_ref(),
        quote! {
            Self {
                #(#blend)*
            }
        },
    );
    Ok(quote! {
        impl #impl_generics ::palantir::widget::Lerp for #name #ty_generics #where_clause {
            #[inline]
            fn lerp(a: &Self, b: &Self, t: f32) -> Self {
                if t == 0.0 {
                    return ::core::clone::Clone::clone(a);
                }
                if t == 1.0 {
                    return ::core::clone::Clone::clone(b);
                }
                #before_lerp_call
                #blended
            }

            #[inline]
            fn lerp_into(a: &Self, b: &Self, t: f32, out: &mut Self) {
                #lerp_into_body
            }
        }
        #ends_into
    })
}

fn expand_text_slots(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let fields: Vec<_> = named_fields(input, "TextSlots")?
        .iter()
        .map(|f| f.ident.as_ref().expect("named fields have idents"))
        .collect();
    Ok(quote! {
        impl #impl_generics crate::theme::TextSlots for #name #ty_generics #where_clause {
            #[inline]
            fn each_text<F>(&self, f: &mut F)
            where
                F: FnMut(crate::theme::ThemeTextRef<'_>),
            {
                #(
                    crate::theme::TextSlots::each_text(&self.#fields, f);
                )*
            }

            #[inline]
            fn for_each_text<F>(&mut self, mut other: ::core::option::Option<&mut Self>, f: &mut F)
            where
                F: FnMut(crate::theme::ThemeText<'_>, ::core::option::Option<crate::theme::ThemeText<'_>>),
            {
                #(
                    crate::theme::TextSlots::for_each_text(
                        &mut self.#fields,
                        other.as_deref_mut().map(|o| &mut o.#fields),
                        f,
                    );
                )*
            }
        }
    })
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = named_fields(input, "Animatable")?;
    let ContainerOptions {
        settle_eps, coerce, ..
    } = ContainerOptions::of(input)?;

    let mut animation: Vec<(&Ident, &Type)> = Vec::new();
    let mut lerp_only: Vec<(&Ident, &Type)> = Vec::new();
    let mut snap: Vec<(&Ident, &Type)> = Vec::new();
    let mut field_eps: Vec<Option<Expr>> = Vec::new();
    for f in fields {
        let ident = f.ident.as_ref().expect("named fields have idents");
        let FieldOptions { kind, settle_eps } = FieldOptions::of(f)?;
        if settle_eps.is_some() && kind != FieldKind::Animated {
            return Err(syn::Error::new_spanned(
                f,
                "`settle_eps` is an animated field's; a `snap` or `lerp_only` field never settles",
            ));
        }
        match kind {
            FieldKind::Animated => {
                animation.push((ident, &f.ty));
                field_eps.push(settle_eps);
            }
            FieldKind::LerpOnly => lerp_only.push((ident, &f.ty)),
            FieldKind::Snap => snap.push((ident, &f.ty)),
        }
    }
    if settle_eps.is_some() && field_eps.iter().any(Option::is_some) {
        return Err(syn::Error::new_spanned(
            input,
            "a struct's `settle_eps` holds every field; a field's `settle_eps` under it is never read",
        ));
    }

    // A lerp-only field keeps `self` in motion, as a snap field does.
    let keep_snap: Vec<TokenStream2> = lerp_only
        .iter()
        .chain(&snap)
        .map(|(f, _)| quote! { #f: ::core::clone::Clone::clone(&self.#f), })
        .collect();

    let sub_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::sub(&self.#f, &other.#f), }
    });

    let add_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::add(&self.#f, &other.#f), }
    });

    let scale_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::scale(&self.#f, k), }
    });

    let magnitude_squared_body = sum_or_zero(
        animation
            .iter()
            .map(|(f, _)| quote! { ::palantir::widget::Animatable::magnitude_squared(&self.#f) }),
    );

    let settle_distance_squared_body = if let Some(eps) = &settle_eps {
        quote! {
            let eps: f32 = #eps;
            ::palantir::widget::Animatable::magnitude_squared(self) / (eps * eps)
        }
    } else {
        sum_or_zero(animation.iter().zip(&field_eps).map(|((f, _), eps)| {
            if let Some(eps) = eps {
                // Parenthesized: a bare block opening the sum would end the expression there.
                quote! {
                    ({
                        let eps: f32 = #eps;
                        ::palantir::widget::Animatable::magnitude_squared(&self.#f) / (eps * eps)
                    })
                }
            } else {
                quote! { ::palantir::widget::Animatable::settle_distance_squared(&self.#f) }
            }
        }))
    };

    let to_motion_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::to_motion(&self.#f), }
    });
    let from_motion_animation = animation.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::from_motion(motion.#f), }
    });
    let motion_snap = lerp_only
        .iter()
        .chain(&snap)
        .map(|(f, _)| quote! { #f: motion.#f, });
    let from_motion_body = coerced(
        coerce.as_ref(),
        quote! {
            Self {
                #(#from_motion_animation)*
                #(#motion_snap)*
            }
        },
    );

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
            fn sub(&self, other: &Self) -> Self {
                Self {
                    #(#sub_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn add(&self, other: &Self) -> Self {
                Self {
                    #(#add_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn scale(&self, k: f32) -> Self {
                Self {
                    #(#scale_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn magnitude_squared(&self) -> f32 {
                #magnitude_squared_body
            }
            #[inline]
            fn settle_distance_squared(&self) -> f32 {
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
            fn to_motion(&self) -> Self {
                Self {
                    #(#to_motion_animation)*
                    #(#keep_snap)*
                }
            }
            #[inline]
            fn from_motion(motion: Self) -> Self {
                #from_motion_body
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

/// `value` through `coerce`, which coerces in place, or as it is with none.
fn coerced(coerce: Option<&ExprPath>, value: TokenStream2) -> TokenStream2 {
    match coerce {
        Some(path) => quote! {
            {
                let mut value = #value;
                #path(&mut value);
                value
            }
        },
        None => value,
    }
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
    coerce: Option<ExprPath>,
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
            .field("coerce", &tokens(self.coerce.as_ref()))
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
                } else if meta.path.is_ident("coerce") {
                    if options.coerce.is_some() {
                        return Err(meta.error("`coerce` given twice"));
                    }
                    options.coerce = Some(meta.value()?.parse::<ExprPath>()?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown #[animate(...)] option on a struct; expected `settle_eps`, `before_lerp` or `coerce`",
                    ))
                }
            })?;
        }
        Ok(options)
    }
}

/// How a field blends and moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldKind {
    /// Forwarded to the field's own `Lerp` and `Animatable` impls.
    Animated,
    /// `#[animate(lerp_only)]`: blends in `lerp`, takes the target at once in motion.
    LerpOnly,
    /// `#[animate(snap)]`: switches halfway through `lerp`, takes the target at once in motion.
    Snap,
}

/// A field's `#[animate(...)]` options.
struct FieldOptions {
    kind: FieldKind,
    settle_eps: Option<Expr>,
}

impl FieldOptions {
    /// Unknown idents and a second option error, so typos fail to compile.
    fn of(f: &Field) -> syn::Result<Self> {
        let mut kind = FieldKind::Animated;
        let mut settle_eps = None;
        for attr in &f.attrs {
            if !attr.path().is_ident("animate") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("settle_eps") {
                    if settle_eps.is_some() {
                        return Err(meta.error("`settle_eps` given twice"));
                    }
                    settle_eps = Some(meta.value()?.parse::<Expr>()?);
                    return Ok(());
                }
                let next = if meta.path.is_ident("snap") {
                    FieldKind::Snap
                } else if meta.path.is_ident("lerp_only") {
                    FieldKind::LerpOnly
                } else {
                    return Err(meta.error(
                        "unknown #[animate(...)] option; expected `snap`, `lerp_only` or `settle_eps`",
                    ));
                };
                if kind != FieldKind::Animated {
                    return Err(meta.error("a field takes one of `snap` and `lerp_only`"));
                }
                kind = next;
                Ok(())
            })?;
        }
        Ok(Self { kind, settle_eps })
    }
}

#[cfg(test)]
mod tests {
    use crate::{expand, expand_lerp, expand_text_slots};
    use syn::{DeriveInput, parse_quote};

    /// Refused inputs and their messages.
    #[test]
    fn refused_inputs_name_the_reason() {
        let shape = "Animatable can only be derived on structs with named fields";
        let option = "unknown #[animate(...)] option; expected `snap`, `lerp_only` or `settle_eps`";
        let container = "unknown #[animate(...)] option on a struct; expected `settle_eps`, `before_lerp` or `coerce`";
        let cases: [(&str, DeriveInput, &str); 13] = [
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
                "a settle tolerance on a snap field",
                parse_quote! { struct S { #[animate(snap, settle_eps = 0.5)] a: f32 } },
                "`settle_eps` is an animated field's; a `snap` or `lerp_only` field never settles",
            ),
            (
                "a field tolerance under a struct tolerance",
                parse_quote! { #[animate(settle_eps = 0.5)] struct S { #[animate(settle_eps = 0.25)] a: f32 } },
                "a struct's `settle_eps` holds every field; a field's `settle_eps` under it is never read",
            ),
            (
                "two coercions",
                parse_quote! { #[animate(coerce = f, coerce = g)] struct S { a: f32 } },
                "`coerce` given twice",
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

    /// `coerce` runs on the blend `lerp` returns and the one `lerp_into` writes, past their exact
    /// ends, and on the value `from_motion` returns, and in no other method.
    #[test]
    fn coerce_wraps_the_blend_and_the_value_back_from_motion() {
        let input: DeriveInput = parse_quote! {
            #[animate(coerce = Self::floored)]
            struct S { a: f32 }
        };
        let lerp = expand_lerp(&input).expect("a lerp").to_string();
        assert_eq!(lerp.matches("Self :: floored (").count(), 2, "{lerp}");
        let animatable = expand(&input).expect("an animatable").to_string();
        assert_eq!(
            animatable.matches("Self :: floored (").count(),
            1,
            "{animatable}"
        );
        let from_motion = animatable.find("fn from_motion").expect("from_motion");
        assert!(
            animatable.find("Self :: floored (").expect("the call") > from_motion,
            "{animatable}"
        );
    }

    /// `Lerp` refuses what `Animatable` refuses, naming itself.
    #[test]
    fn lerp_names_itself_when_it_refuses() {
        let error = expand_lerp(&parse_quote! { enum E { A } }).expect_err("an enum");
        assert_eq!(
            error.to_string(),
            "Lerp can only be derived on structs with named fields"
        );
    }

    /// The walk visits every field, in field order, and refuses what is not a struct with named
    /// fields.
    #[test]
    fn text_slots_walks_every_field_in_order() {
        let walked = expand_text_slots(&parse_quote! {
            struct S { a: f32, #[animate(snap)] b: u8, c: u16 }
        })
        .expect("a text walk")
        .to_string();
        assert!(walked.contains("TextSlots for S"), "{walked}");
        let visits: Vec<usize> = ["self . a", "self . b", "self . c"]
            .iter()
            .map(|field| walked.find(field).expect(field))
            .collect();
        assert!(visits.is_sorted(), "{walked}");
        assert_eq!(
            walked.matches("for_each_text (").count(),
            3,
            "one call a field"
        );

        let error = expand_text_slots(&parse_quote! { struct T(f32); }).expect_err("tuple");
        assert_eq!(
            error.to_string(),
            "TextSlots can only be derived on structs with named fields"
        );
    }
}
