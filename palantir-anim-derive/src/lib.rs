//! Derive macro for `palantir::widget::Animatable`. Walks each field of a
//! struct: animated fields call into the inner `Animatable` impl;
//! fields marked `#[animate(snap)]` are excluded from arithmetic
//! (lerp returns target's value, sub/add/scale/zero preserve `self`'s
//! or pick a default, magnitude_squared contributes 0). Dynamic
//! spring normalization forwards through animated fields only, and so
//! does the settle distance, unless the struct names a tolerance of its
//! own.
//!
//! Re-exported as `palantir::widget::Animatable` (the derive shares its name
//! with the trait, by Rust convention).

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Expr, Field, Fields, Ident, Type, parse_macro_input};

/// `#[derive(Animatable)]` on a struct with named fields.
///
/// Per-field attribute `#[animate(snap)]` (or its alias
/// `#[animate(skip)]`) marks the field as non-animated: lerp returns
/// the target's value, spring math noops on it, and `magnitude`
/// excludes it. Useful for fields whose continuous interpolation is
/// expensive (font sizes invalidating shape caches), aesthetically
/// off (corner radii morphing across states), or simply not
/// `Animatable` (`Spacing`, etc.).
///
/// Struct attribute `#[animate(settle_eps = EXPR)]` gives the whole value
/// one settle tolerance, `EXPR`, in its own unit: the settle distance is
/// then its magnitude over `EXPR²`. Without it, the settle distance is
/// the sum of the animated fields' own, each in its own unit.
#[proc_macro_derive(Animatable, attributes(animate))]
pub fn derive_animatable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The `Animatable` impl for `input`, or the error the derive reports in
/// its place.
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

    let settle_eps = container_settle_eps(input)?;

    let mut anim: Vec<(&Ident, &Type)> = Vec::new();
    let mut snap: Vec<(&Ident, &Type)> = Vec::new();
    for f in fields {
        let Some(ident) = f.ident.as_ref() else {
            continue;
        };
        if classify_field(f)? {
            snap.push((ident, &f.ty));
        } else {
            anim.push((ident, &f.ty));
        }
    }

    let lerp_anim = anim.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::lerp(a.#f, b.#f, t), }
    });
    let lerp_snap = snap.iter().map(|(f, _)| {
        quote! { #f: b.#f, }
    });

    let sub_anim = anim.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::sub(self.#f, other.#f), }
    });
    let sub_snap = snap.iter().map(|(f, _)| {
        quote! { #f: self.#f, }
    });

    let add_anim = anim.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::add(self.#f, other.#f), }
    });
    let add_snap = snap.iter().map(|(f, _)| {
        quote! { #f: self.#f, }
    });

    let scale_anim = anim.iter().map(|(f, _)| {
        quote! { #f: ::palantir::widget::Animatable::scale(self.#f, k), }
    });
    let scale_snap = snap.iter().map(|(f, _)| {
        quote! { #f: self.#f, }
    });

    let mag_sq_terms: Vec<TokenStream2> = anim
        .iter()
        .map(|(f, _)| quote! { ::palantir::widget::Animatable::magnitude_squared(self.#f) })
        .collect();
    let magnitude_squared_body = if mag_sq_terms.is_empty() {
        quote! { 0.0_f32 }
    } else {
        quote! { #(#mag_sq_terms)+* }
    };

    let settle_distance_squared_body = match &settle_eps {
        Some(eps) => quote! {
            let eps: f32 = #eps;
            ::palantir::widget::Animatable::magnitude_squared(self) / (eps * eps)
        },
        None => {
            let terms: Vec<TokenStream2> = anim
                .iter()
                .map(|(f, _)| {
                    quote! { ::palantir::widget::Animatable::settle_distance_squared(self.#f) }
                })
                .collect();
            if terms.is_empty() {
                quote! { 0.0_f32 }
            } else {
                quote! { #(#terms)+* }
            }
        }
    };

    let zero_anim = anim.iter().map(|(f, ty)| {
        quote! { #f: <#ty as ::palantir::widget::Animatable>::zero(), }
    });
    let zero_snap = snap.iter().map(|(f, ty)| {
        quote! { #f: <#ty as ::core::default::Default>::default(), }
    });
    let normalize_for_spring_anim = anim.iter().map(|(f, ty)| {
        quote! {
            <#ty as ::palantir::widget::Animatable>::normalize_for_spring(
                &mut self.#f,
                &target.#f,
                &mut velocity.#f,
            );
        }
    });

    // `#[inline]` on each method: Animatable is a tight math trait
    // called per frame per animation, often across crate boundaries
    // (palantir's `tick` calling derived impls in user code). Forces
    // availability for cross-crate inlining.
    let expanded = quote! {
        impl #impl_generics ::palantir::widget::Animatable for #name #ty_generics #where_clause {
            #[inline]
            fn lerp(a: Self, b: Self, t: f32) -> Self {
                Self {
                    #(#lerp_anim)*
                    #(#lerp_snap)*
                }
            }
            #[inline]
            fn sub(self, other: Self) -> Self {
                Self {
                    #(#sub_anim)*
                    #(#sub_snap)*
                }
            }
            #[inline]
            fn add(self, other: Self) -> Self {
                Self {
                    #(#add_anim)*
                    #(#add_snap)*
                }
            }
            #[inline]
            fn scale(self, k: f32) -> Self {
                Self {
                    #(#scale_anim)*
                    #(#scale_snap)*
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
                    #(#zero_anim)*
                    #(#zero_snap)*
                }
            }
            #[inline]
            fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
                let _ = (&*self, target, &*velocity);
                #(#normalize_for_spring_anim)*
            }
        }
    };

    Ok(expanded)
}

/// The struct's own `#[animate(settle_eps = EXPR)]`, if it names one.
/// Errors on any other option there, as [`classify_field`] does on a
/// field, and on a second tolerance, which would otherwise win silently.
fn container_settle_eps(input: &DeriveInput) -> syn::Result<Option<Expr>> {
    let mut eps = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("animate") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("settle_eps") {
                if eps.is_some() {
                    return Err(meta.error("`settle_eps` given twice"));
                }
                eps = Some(meta.value()?.parse::<Expr>()?);
                Ok(())
            } else {
                Err(meta.error("unknown #[animate(...)] option on a struct; expected `settle_eps`"))
            }
        })?;
    }
    Ok(eps)
}

/// Returns `Ok(true)` if `#[animate(snap)]` (or `skip`) is set on the
/// field, `Ok(false)` otherwise. Errors on unrecognised idents inside
/// `#[animate(...)]` so typos like `#[animate(snip)]` fail loud at
/// compile time instead of silently animating the field.
fn classify_field(f: &Field) -> syn::Result<bool> {
    let mut snap = false;
    for attr in &f.attrs {
        if !attr.path().is_ident("animate") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("snap") || meta.path.is_ident("skip") {
                snap = true;
                Ok(())
            } else {
                Err(meta.error("unknown #[animate(...)] option; expected `snap` or `skip`"))
            }
        })?;
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use crate::expand;
    use syn::{DeriveInput, parse_quote};

    /// Each input the derive refuses, and the message it refuses with:
    /// a shape with no named fields to walk, and an `#[animate(..)]`
    /// option that is neither `snap` nor its alias `skip` — a typo there
    /// would otherwise animate the field silently.
    #[test]
    fn refused_inputs_name_the_reason() {
        let shape = "Animatable can only be derived on structs with named fields";
        let option = "unknown #[animate(...)] option; expected `snap` or `skip`";
        let container = "unknown #[animate(...)] option on a struct; expected `settle_eps`";
        let cases: [(&str, DeriveInput, &str); 6] = [
            ("enum", parse_quote! { enum E { A } }, shape),
            ("tuple struct", parse_quote! { struct T(f32); }, shape),
            ("unit struct", parse_quote! { struct U; }, shape),
            (
                "typo",
                parse_quote! { struct S { #[animate(snip)] a: f32 } },
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
        ];
        for (label, input, message) in cases {
            let error = expand(&input).expect_err(label);
            assert_eq!(error.to_string(), message, "{label}");
        }
    }
}
