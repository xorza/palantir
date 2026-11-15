//! `#[derive(Lerp)]`'s expansion.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::DeriveInput;

use crate::options::{ContainerOptions, FieldKind, FieldOptions, coerced, named_fields};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
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
    for (f, options) in fields.iter().zip(FieldOptions::of_each(fields)?) {
        let ident = f.ident.as_ref().expect("named fields have idents");
        let kind = options.kind;
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

#[cfg(test)]
mod tests {
    use crate::lerp;
    use syn::parse_quote;

    /// `Lerp` refuses what `Animatable` refuses, naming itself.
    #[test]
    fn lerp_names_itself_when_it_refuses() {
        let error = lerp::expand(&parse_quote! { enum E { A } }).expect_err("an enum");
        assert_eq!(
            error.to_string(),
            "Lerp can only be derived on structs with named fields"
        );
    }
}
