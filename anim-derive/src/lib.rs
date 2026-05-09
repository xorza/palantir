//! Derive macro for `palantir::Animatable`. Walks each field of a
//! struct: animated fields call into the inner `Animatable` impl;
//! fields marked `#[animate(snap)]` are excluded from arithmetic
//! (lerp returns target's value, sub/add/scale/zero preserve `self`'s
//! or pick a default, magnitude contributes 0).
//!
//! Re-exported as `palantir::Animatable` (the derive shares its name
//! with the trait, by Rust convention).

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Field, Fields, Ident, parse_macro_input};

/// `#[derive(Animatable)]` on a struct with named fields.
///
/// Per-field attribute `#[animate(snap)]` (or its alias
/// `#[animate(skip)]`) marks the field as non-animated: lerp returns
/// the target's value, spring math noops on it, and `magnitude`
/// excludes it. Useful for fields whose continuous interpolation is
/// expensive (font sizes invalidating shape caches), aesthetically
/// off (corner radii morphing across states), or simply not
/// `Animatable` (`Spacing`, etc.).
#[proc_macro_derive(Animatable, attributes(animate))]
pub fn derive_animatable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => {
            return syn::Error::new_spanned(
                &input,
                "Animatable can only be derived on structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let mut anim: Vec<&Ident> = Vec::new();
    let mut snap: Vec<&Ident> = Vec::new();
    for f in fields {
        let ident = match f.ident.as_ref() {
            Some(i) => i,
            None => continue,
        };
        if is_snap(f) {
            snap.push(ident);
        } else {
            anim.push(ident);
        }
    }

    let lerp_anim = anim.iter().map(|f| {
        quote! { #f: ::palantir::Animatable::lerp(a.#f, b.#f, t), }
    });
    let lerp_snap = snap.iter().map(|f| {
        quote! { #f: b.#f, }
    });

    let sub_anim = anim.iter().map(|f| {
        quote! { #f: ::palantir::Animatable::sub(self.#f, other.#f), }
    });
    let sub_snap = snap.iter().map(|f| {
        quote! { #f: self.#f, }
    });

    let add_anim = anim.iter().map(|f| {
        quote! { #f: ::palantir::Animatable::add(self.#f, other.#f), }
    });
    let add_snap = snap.iter().map(|f| {
        quote! { #f: self.#f, }
    });

    let scale_anim = anim.iter().map(|f| {
        quote! { #f: ::palantir::Animatable::scale(self.#f, k), }
    });
    let scale_snap = snap.iter().map(|f| {
        quote! { #f: self.#f, }
    });

    let mag_terms: Vec<TokenStream2> = anim
        .iter()
        .map(|f| {
            let m: TokenStream2 = quote! { ::palantir::Animatable::magnitude(self.#f) };
            quote! { (#m * #m) }
        })
        .collect();
    let magnitude_body = if mag_terms.is_empty() {
        quote! { 0.0_f32 }
    } else {
        quote! {
            {
                let sum: f32 = #(#mag_terms)+*;
                sum.sqrt()
            }
        }
    };

    let zero_anim = anim.iter().map(|f| {
        let ty = field_ty(fields, f);
        quote! { #f: <#ty as ::palantir::Animatable>::zero(), }
    });
    let zero_snap = snap.iter().map(|f| {
        let ty = field_ty(fields, f);
        quote! { #f: <#ty as ::core::default::Default>::default(), }
    });

    // `#[inline]` on each method: Animatable is a tight math trait
    // called per frame per animation, often across crate boundaries
    // (palantir's `tick` calling derived impls in user code). Forces
    // availability for cross-crate inlining.
    let expanded = quote! {
        impl #impl_generics ::palantir::Animatable for #name #ty_generics #where_clause {
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
            fn magnitude(self) -> f32 {
                #magnitude_body
            }
            #[inline]
            fn zero() -> Self {
                Self {
                    #(#zero_anim)*
                    #(#zero_snap)*
                }
            }
        }
    };

    expanded.into()
}

fn is_snap(f: &Field) -> bool {
    let mut snap = false;
    for attr in &f.attrs {
        if !attr.path().is_ident("animate") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("snap") || meta.path.is_ident("skip") {
                snap = true;
            }
            Ok(())
        });
    }
    snap
}

fn field_ty<'a>(
    fields: &'a syn::punctuated::Punctuated<Field, syn::Token![,]>,
    name: &Ident,
) -> &'a syn::Type {
    for f in fields {
        if f.ident.as_ref() == Some(name) {
            return &f.ty;
        }
    }
    unreachable!("field {} not found", name)
}
