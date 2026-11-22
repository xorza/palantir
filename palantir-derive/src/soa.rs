//! `#[derive(Soa)]`'s expansion.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DeriveInput, Ident, Index, Type};

use crate::options::named_fields;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let row = &input.ident;
    let vis = &input.vis;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Soa can only be derived on a struct with no generics",
        ));
    }
    let columns = columns_name(input)?;
    let columns_mut = format_ident!("{columns}Mut");
    let columns_iter = format_ident!("{columns}Iter");
    let fields = named_fields(input, "Soa")?;
    if fields.is_empty() {
        return Err(syn::Error::new_spanned(
            input,
            "Soa needs a struct with a field",
        ));
    }
    let idents: Vec<&Ident> = fields
        .iter()
        .map(|f| f.ident.as_ref().expect("named fields have idents"))
        .collect();
    let tys: Vec<&Type> = fields.iter().map(|f| &f.ty).collect();
    let at: Vec<Index> = (0..fields.len()).map(Index::from).collect();

    let doc = format!(
        "The rows of [`{row}`] by column: one buffer a field, all of one length, kept once."
    );
    let doc_mut = format!("Every column of a [`{columns}`] at once, each borrowed mutably.");
    let doc_iter = format!("The rows of a [`{columns}`] in order, each copied out of its columns.");
    let first = idents[0];
    let first_ty = tys[0];
    let zipped = idents[1..]
        .iter()
        .fold(quote! { #first }, |acc, f| quote! { (#acc, #f) });
    let zip_iter = idents[1..].iter().fold(
        quote! { self.#first().iter() },
        |acc, f| quote! { ::core::iter::Iterator::zip(#acc, self.#f().iter()) },
    );
    let zip_ty = tys[1..].iter().fold(
        quote! { ::core::slice::Iter<'a, #first_ty> },
        |acc, ty| quote! { ::core::iter::Zip<#acc, ::core::slice::Iter<'a, #ty>> },
    );
    let assemble = quote! { |#zipped| #row { #( #idents: *#idents, )* } };

    // Each column is a `Vec` used for its buffer alone: its own length stays 0 outside `grow`,
    // the one `len` covers every column, and `cap` is the least capacity among them. So a push
    // checks one capacity and stores one length, and every column's slice is `len` long, which
    // lets the optimizer drop a second column's bounds check where the first's held. The `Vec`
    // still owns, grows and frees each buffer. A column holds only `Copy` values, so no element
    // ever needs a drop.
    Ok(quote! {
        #[doc = #doc]
        #vis struct #columns {
            buffers: ( #( ::std::vec::Vec<#tys>, )* ),
            len: usize,
            cap: usize,
        }

        const _: () = {
            const fn copy<T: ::core::marker::Copy>() {}
            #( copy::<#tys>(); )*
        };

        impl ::core::default::Default for #columns {
            fn default() -> Self {
                Self {
                    buffers: ( #( ::std::vec::Vec::<#tys>::new(), )* ),
                    len: 0,
                    cap: 0,
                }
            }
        }

        impl ::core::clone::Clone for #columns {
            fn clone(&self) -> Self {
                let mut clone = Self::default();
                clone.reserve(self.len);
                // SAFETY: `self`'s first `len` slots hold rows, and `clone`'s buffers hold at
                // least `len` slots (`reserve`), in allocations of their own.
                #(
                    unsafe {
                        ::core::ptr::copy_nonoverlapping(
                            self.buffers.#at.as_ptr(),
                            clone.buffers.#at.as_mut_ptr(),
                            self.len,
                        );
                    }
                )*
                clone.len = self.len;
                clone
            }
        }

        impl ::core::fmt::Debug for #columns {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(::core::stringify!(#columns))
                    #( .field(::core::stringify!(#idents), &self.#idents()) )*
                    .finish()
            }
        }

        #[doc = #doc_mut]
        #[derive(Debug)]
        #vis struct #columns_mut<'a> {
            #( #vis #idents: &'a mut [#tys], )*
        }

        #[doc = #doc_iter]
        #[derive(Clone, Debug)]
        #vis struct #columns_iter<'a> {
            zipped: #zip_ty,
        }

        impl ::core::iter::Iterator for #columns_iter<'_> {
            type Item = #row;

            #[inline]
            fn next(&mut self) -> ::core::option::Option<#row> {
                self.zipped.next().map(#assemble)
            }

            #[inline]
            fn size_hint(&self) -> (usize, ::core::option::Option<usize>) {
                self.zipped.size_hint()
            }
        }

        impl ::core::iter::DoubleEndedIterator for #columns_iter<'_> {
            #[inline]
            fn next_back(&mut self) -> ::core::option::Option<#row> {
                self.zipped.next_back().map(#assemble)
            }
        }

        impl ::core::iter::ExactSizeIterator for #columns_iter<'_> {}

        impl ::core::iter::FusedIterator for #columns_iter<'_> {}

        impl<'a> ::core::iter::IntoIterator for &'a #columns {
            type Item = #row;
            type IntoIter = #columns_iter<'a>;

            #[inline]
            fn into_iter(self) -> #columns_iter<'a> {
                #columns_iter { zipped: #zip_iter }
            }
        }

        impl ::core::iter::FromIterator<#row> for #columns {
            fn from_iter<I: ::core::iter::IntoIterator<Item = #row>>(rows: I) -> Self {
                let mut columns = Self::default();
                ::core::iter::Extend::extend(&mut columns, rows);
                columns
            }
        }

        impl<const N: usize> ::core::convert::From<[#row; N]> for #columns {
            fn from(rows: [#row; N]) -> Self {
                ::core::iter::FromIterator::from_iter(rows)
            }
        }

        impl ::core::iter::Extend<#row> for #columns {
            fn extend<I: ::core::iter::IntoIterator<Item = #row>>(&mut self, rows: I) {
                let rows = ::core::iter::IntoIterator::into_iter(rows);
                self.reserve(::core::iter::Iterator::size_hint(&rows).0);
                for row in rows {
                    self.push(row);
                }
            }
        }

        #[allow(dead_code, reason = "each row type calls only the methods it needs")]
        impl #columns {
            /// How many rows the columns hold.
            #[inline]
            #vis const fn len(&self) -> usize {
                self.len
            }

            /// Whether the columns hold no row.
            #[inline]
            #vis const fn is_empty(&self) -> bool {
                self.len == 0
            }

            /// Append `row`, a field to each column, behind one capacity check for them all.
            #[inline]
            #vis fn push(&mut self, row: #row) {
                if self.len == self.cap {
                    self.grow(1);
                }
                // SAFETY: every buffer holds `cap > len` slots (`set_cap`), so slot `len` of
                // each lies inside its allocation, and no slice covers it until `len` grows.
                #(
                    unsafe { self.buffers.#at.as_mut_ptr().add(self.len).write(row.#idents) };
                )*
                self.len += 1;
            }

            /// Put `row` at `idx`, shifting every row after it up one.
            ///
            /// # Panics
            ///
            /// Panics if `idx` is past [`Self::len`].
            #vis fn insert(&mut self, idx: usize, row: #row) {
                ::core::assert!(idx <= self.len, "insert at {idx} past {} rows", self.len);
                if self.len == self.cap {
                    self.grow(1);
                }
                // SAFETY: as in `push`, slot `len` lies inside every buffer, so the rows from
                // `idx` move up one within it, and slot `idx` takes the new row.
                #(
                    unsafe {
                        let buffer = self.buffers.#at.as_mut_ptr();
                        ::core::ptr::copy(buffer.add(idx), buffer.add(idx + 1), self.len - idx);
                        buffer.add(idx).write(row.#idents);
                    }
                )*
                self.len += 1;
            }

            /// Make room for `additional` more rows in every column.
            #vis fn reserve(&mut self, additional: usize) {
                if self.cap - self.len < additional {
                    self.grow(additional);
                }
            }

            /// Grow every buffer to hold `additional` more rows, at least doubling, as `Vec`
            /// grows; out of the push's line.
            #[cold]
            #[inline(never)]
            fn grow(&mut self, additional: usize) {
                let target = self
                    .len
                    .checked_add(additional)
                    .expect("a row count past usize")
                    .max(self.cap.saturating_mul(2))
                    .max(4);
                // SAFETY: the first `len` slots of every buffer hold rows and lie inside it, so
                // the buffer may claim them while it reallocates, which keeps the elements it
                // holds, and gives them back after.
                #(
                    unsafe { self.buffers.#at.set_len(self.len) };
                    self.buffers.#at.reserve_exact(target - self.len);
                    unsafe { self.buffers.#at.set_len(0) };
                )*
                self.set_cap();
            }

            /// `cap`, the least capacity of any buffer.
            fn set_cap(&mut self) {
                self.cap = [#( self.buffers.#at.capacity(), )*]
                    .into_iter()
                    .min()
                    .expect("a row has a field");
            }

            /// Keep the first `len` rows, as `Vec::truncate` does.
            #[inline]
            #vis const fn truncate(&mut self, len: usize) {
                if len < self.len {
                    self.len = len;
                }
            }

            /// Drop every row, keeping each column's capacity.
            #[inline]
            #vis const fn clear(&mut self) {
                self.len = 0;
            }

            /// Each row in order, its fields copied out of their columns.
            #[inline]
            #vis fn iter(&self) -> #columns_iter<'_> {
                ::core::iter::IntoIterator::into_iter(self)
            }

            /// Every column at once, each borrowed mutably: the one way to write rows in place, so a
            /// pass that writes several columns borrows them all together.
            #[inline]
            #vis const fn columns_mut(&mut self) -> #columns_mut<'_> {
                // SAFETY: the first `len` slots of every buffer hold rows, inside its own
                // allocation, so the slices do not overlap, and `&mut self` keeps them unique.
                unsafe {
                    #columns_mut {
                        #(
                            #idents: ::core::slice::from_raw_parts_mut(
                                self.buffers.#at.as_mut_ptr(),
                                self.len,
                            ),
                        )*
                    }
                }
            }

            #(
                #[doc = concat!("The `", stringify!(#idents), "` column.")]
                #[inline]
                #vis const fn #idents(&self) -> &[#tys] {
                    // SAFETY: the first `len` slots of every buffer hold rows (`push`, `insert`,
                    // `clone`), inside its allocation.
                    unsafe { ::core::slice::from_raw_parts(self.buffers.#at.as_ptr(), self.len) }
                }
            )*
        }
    })
}

/// The name `#[soa(Name)]` gives the columns type.
fn columns_name(input: &DeriveInput) -> syn::Result<Ident> {
    let mut name = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("soa")) {
        if name.is_some() {
            return Err(syn::Error::new_spanned(attr, "one #[soa(Name)] attribute"));
        }
        name = Some(attr.parse_args::<Ident>()?);
    }
    name.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "Soa needs #[soa(Name)] for its columns type")
    })
}

#[cfg(test)]
mod tests {
    use crate::soa;
    use syn::parse_quote;

    /// The columns type takes the attribute's name, one buffer a field, and an accessor a field;
    /// a row with generics, a tuple struct, no field or no name is refused.
    #[test]
    fn columns_take_their_name_and_a_buffer_a_field() {
        let expanded = soa::expand(&parse_quote! {
            #[soa(Rows)]
            pub(crate) struct Row { pub(crate) a: u32, pub(crate) b: f32 }
        })
        .expect("columns")
        .to_string();
        assert!(expanded.contains("pub (crate) struct Rows {"), "{expanded}");
        assert!(
            expanded.contains(
                "buffers : (:: std :: vec :: Vec < u32 > , :: std :: vec :: Vec < f32 > ,)"
            ),
            "{expanded}"
        );
        assert!(
            expanded.contains("pub (crate) struct RowsMut < 'a >"),
            "{expanded}"
        );
        assert!(
            expanded.contains("const fn a (& self) -> & [u32]"),
            "{expanded}"
        );
        assert!(
            expanded.contains("const fn b (& self) -> & [f32]"),
            "{expanded}"
        );

        let refused = [
            (
                parse_quote! { #[soa(Rows)] struct Row<T> { a: T } },
                "Soa can only be derived on a struct with no generics",
            ),
            (
                parse_quote! { #[soa(Rows)] struct Row(u32); },
                "Soa can only be derived on structs with named fields",
            ),
            (
                parse_quote! { #[soa(Rows)] struct Row {} },
                "Soa needs a struct with a field",
            ),
            (
                parse_quote! { struct Row { a: u32 } },
                "Soa needs #[soa(Name)] for its columns type",
            ),
        ];
        for (input, message) in refused {
            let error = soa::expand(&input).expect_err(message);
            assert_eq!(error.to_string(), message);
        }
    }
}
