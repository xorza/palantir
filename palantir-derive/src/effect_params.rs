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
    if !is_repr_c(input)? {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "EffectParams needs `#[repr(C)]`, so its field offsets are the ones checked against \
             the WGSL struct",
        ));
    }
    let name = &input.ident;
    let fields = named_fields(input, "EffectParams")?.iter().map(|field| {
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
                    // `align(N)` and `packed(N)` carry an argument this check does not read.
                    let _skipped: TokenStream2 = meta.input.parse()?;
                }
                Ok(())
            })?;
        }
    }
    Ok(repr_c)
}
