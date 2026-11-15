//! `#[derive(ThemeWalk)]`'s expansion.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::DeriveInput;

use crate::options::named_fields;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let fields: Vec<_> = named_fields(input, "ThemeWalk")?
        .iter()
        .map(|f| f.ident.as_ref().expect("named fields have idents"))
        .collect();
    Ok(quote! {
        impl #impl_generics crate::theme::ThemeWalk for #name #ty_generics #where_clause {
            #[inline]
            fn each_text<F>(&self, f: &mut F)
            where
                F: FnMut(crate::theme::ThemeTextRef<'_>),
            {
                #(
                    crate::theme::ThemeWalk::each_text(&self.#fields, f);
                )*
            }

            #[inline]
            fn each_text_mut<F>(&mut self, f: &mut F)
            where
                F: FnMut(crate::theme::ThemeText<'_>),
            {
                #(
                    crate::theme::ThemeWalk::each_text_mut(&mut self.#fields, f);
                )*
            }

            #[inline]
            fn zip_text_mut<F>(&mut self, other: &mut Self, f: &mut F)
            where
                F: FnMut(crate::theme::ThemeText<'_>, crate::theme::ThemeText<'_>),
            {
                #(
                    crate::theme::ThemeWalk::zip_text_mut(&mut self.#fields, &mut other.#fields, f);
                )*
            }

            #[inline]
            fn each_press<F>(&self, f: &mut F)
            where
                F: FnMut(&crate::theme::look::press_effect::PressEffect),
            {
                #(
                    crate::theme::ThemeWalk::each_press(&self.#fields, f);
                )*
            }

            #[inline]
            fn each_material<F>(&self, f: &mut F)
            where
                F: FnMut(&crate::primitives::paint::material::Material),
            {
                #(
                    crate::theme::ThemeWalk::each_material(&self.#fields, f);
                )*
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::theme_walk;
    use syn::parse_quote;

    /// The walk visits every field, in field order, and refuses what is not a struct with named
    /// fields.
    #[test]
    fn text_slots_walks_every_field_in_order() {
        let walked = theme_walk::expand(&parse_quote! {
            struct S { a: f32, #[animate(snap)] b: u8, c: u16 }
        })
        .expect("a text walk")
        .to_string();
        assert!(walked.contains("ThemeWalk for S"), "{walked}");
        let visits: Vec<usize> = ["self . a", "self . b", "self . c"]
            .iter()
            .map(|field| walked.find(field).expect(field))
            .collect();
        assert!(visits.is_sorted(), "{walked}");
        for walk in ["each_text (", "each_text_mut (", "zip_text_mut ("] {
            assert_eq!(walked.matches(walk).count(), 3, "one {walk} call a field");
        }

        let error = theme_walk::expand(&parse_quote! { struct T(f32); }).expect_err("tuple");
        assert_eq!(
            error.to_string(),
            "ThemeWalk can only be derived on structs with named fields"
        );
    }
}
