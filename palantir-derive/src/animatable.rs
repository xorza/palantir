//! `#[derive(Animatable)]`'s expansion.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Expr, Ident, Type};

use crate::options::{ContainerOptions, Errors, FieldKind, FieldOptions, coerced, named_fields};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
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
    let mut errors = Errors::default();
    for (f, FieldOptions { kind, settle_eps }) in fields.iter().zip(FieldOptions::of_each(fields)?)
    {
        let ident = f.ident.as_ref().expect("named fields have idents");
        if settle_eps.is_some() && kind != FieldKind::Animated {
            errors.push(syn::Error::new_spanned(
                f,
                "`settle_eps` is an animated field's; a `snap` or `lerp_only` field never settles",
            ));
            continue;
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
        errors.push(syn::Error::new_spanned(
            input,
            "a struct's `settle_eps` holds every field; a field's `settle_eps` under it is never read",
        ));
    }
    errors.or(())?;

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

/// `a + b + …` over `terms`, or `0.0_f32` for none.
fn sum_or_zero(terms: impl IntoIterator<Item = TokenStream2>) -> TokenStream2 {
    let terms: Vec<TokenStream2> = terms.into_iter().collect();
    if terms.is_empty() {
        quote! { 0.0_f32 }
    } else {
        quote! { #(#terms)+* }
    }
}

#[cfg(test)]
mod tests {
    use crate::{animatable, lerp};
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
            let error = animatable::expand(&input).expect_err(label);
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
        let lerp = lerp::expand(&input).expect("a lerp").to_string();
        assert_eq!(lerp.matches("Self :: floored (").count(), 2, "{lerp}");
        let animatable = animatable::expand(&input)
            .expect("an animatable")
            .to_string();
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

    /// Every field's error reaches the compile, not the first alone.
    #[test]
    fn each_field_reports_its_own_error() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[animate(snip)] a: f32,
                #[animate(snap, settle_eps = 0.5)] b: f32,
                #[animate(skip)] c: f32,
            }
        };
        let option = "unknown #[animate(...)] option; expected `snap`, `lerp_only` or `settle_eps`";
        let errors: Vec<String> = lerp::expand(&input)
            .expect_err("two typos")
            .into_iter()
            .map(|error| error.to_string())
            .collect();
        assert_eq!(errors, [option, option]);
        let errors: Vec<String> = animatable::expand(&input)
            .expect_err("two typos")
            .into_iter()
            .map(|error| error.to_string())
            .collect();
        assert_eq!(
            errors,
            [option, option],
            "the options fail before the tolerance check"
        );
        let tolerances: DeriveInput = parse_quote! {
            #[animate(settle_eps = 1.0)]
            struct S {
                #[animate(snap, settle_eps = 0.5)] a: f32,
                #[animate(settle_eps = 0.5)] b: f32,
            }
        };
        let errors: Vec<String> = animatable::expand(&tolerances)
            .expect_err("two misplaced tolerances")
            .into_iter()
            .map(|error| error.to_string())
            .collect();
        assert_eq!(
            errors,
            [
                "`settle_eps` is an animated field's; a `snap` or `lerp_only` field never settles",
                "a struct's `settle_eps` holds every field; a field's `settle_eps` under it is never read",
            ]
        );
    }
}
