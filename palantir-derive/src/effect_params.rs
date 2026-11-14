//! `#[derive(EffectParams)]`: an effect's parameter struct, described field by field for the
//! check against its WGSL struct.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::ext::IdentExt as _;
use syn::token::Paren;
use syn::{DeriveInput, Meta};

use crate::named_fields;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "EffectParams can only be derived on a struct with no generic parameters",
        ));
    }
    let named = named_fields(input, "EffectParams")?;
    if !is_repr_c(input)? {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "EffectParams needs `#[repr(C)]`, so its field offsets are the ones checked against \
             the WGSL struct",
        ));
    }
    let name = &input.ident;
    let fields = named.iter().map(|field| {
        let ident = field.ident.as_ref().expect("named fields have idents");
        let field_name = ident.unraw().to_string();
        let ty = &field.ty;
        quote! {
            ::palantir::EffectParam {
                name: #field_name,
                offset: ::core::mem::offset_of!(#name, #ident),
                ty: <#ty as ::palantir::ParamValue>::TYPE,
            },
        }
    });
    Ok(quote! {
        impl ::palantir::EffectParams for #name {
            const FIELDS: &'static [::palantir::EffectParam] = &[#(#fields)*];
        }
    })
}

/// Whether the struct carries `#[repr(C)]`, alone or beside an alignment.
fn is_repr_c(input: &DeriveInput) -> syn::Result<bool> {
    let mut repr_c = false;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("repr"))
    {
        if let Meta::List(_) = &attr.meta {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("C") {
                    repr_c = true;
                } else if meta.input.peek(Paren) {
                    // `align(N)` and `packed(N)` carry an argument this check does not read. The
                    // group alone: a `TokenStream` parsed off the list takes the rest of it, a
                    // `C` after the argument included.
                    let argument;
                    syn::parenthesized!(argument in meta.input);
                    let _skipped: TokenStream2 = argument.parse()?;
                }
                Ok(())
            })?;
        }
    }
    Ok(repr_c)
}

#[cfg(test)]
mod tests {
    use crate::effect_params::expand;
    use syn::{DeriveInput, parse_quote};

    /// `repr(C)` is found wherever it stands in the list, and a struct without it is refused.
    #[test]
    fn repr_c_is_read_beside_an_alignment_in_either_order() {
        let needs_repr_c = "EffectParams needs `#[repr(C)]`, so its field offsets are the ones \
                            checked against the WGSL struct";
        let cases: [(&str, DeriveInput, Option<&str>); 7] = [
            (
                "alone",
                parse_quote! { #[repr(C)] struct S { a: f32 } },
                None,
            ),
            (
                "before an alignment",
                parse_quote! { #[repr(C, align(16))] struct S { a: f32 } },
                None,
            ),
            (
                "after an alignment",
                parse_quote! { #[repr(align(16), C)] struct S { a: f32 } },
                None,
            ),
            (
                "after a packing",
                parse_quote! { #[repr(packed(2), C)] struct S { a: f32 } },
                None,
            ),
            (
                "in a second attribute",
                parse_quote! { #[repr(align(16))] #[repr(C)] struct S { a: f32 } },
                None,
            ),
            (
                "transparent",
                parse_quote! { #[repr(transparent)] struct S { a: f32 } },
                Some(needs_repr_c),
            ),
            (
                "an alignment alone",
                parse_quote! { #[repr(align(16))] struct S { a: f32 } },
                Some(needs_repr_c),
            ),
        ];
        for (label, input, refusal) in cases {
            let message = expand(&input).err().map(|error| error.to_string());
            assert_eq!(message.as_deref(), refusal, "{label}");
        }
    }

    /// The shape is named before the layout: a tuple struct is not told to add `repr(C)` first.
    #[test]
    fn a_struct_without_named_fields_is_refused_for_its_shape() {
        let shape = "EffectParams can only be derived on structs with named fields";
        let cases: [(&str, DeriveInput); 3] = [
            ("tuple struct", parse_quote! { struct T(f32); }),
            ("unit struct", parse_quote! { struct U; }),
            ("enum", parse_quote! { enum E { A } }),
        ];
        for (label, input) in cases {
            let error = expand(&input).expect_err(label);
            assert_eq!(error.to_string(), shape, "{label}");
        }
    }
}
