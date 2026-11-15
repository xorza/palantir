//! The `#[animate(...)]` options `Lerp` and `Animatable` read, and the named fields every derive
//! takes.

use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use std::fmt;
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{Data, DataStruct, DeriveInput, Expr, ExprPath, Field, Fields};

/// The named fields of a struct, or the error each derive gives any other input.
pub(crate) fn named_fields<'a>(
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

/// `value` through `coerce`, which coerces in place, or as it is with none.
pub(crate) fn coerced(coerce: Option<&ExprPath>, value: TokenStream2) -> TokenStream2 {
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

/// The struct's `#[animate(...)]` options.
#[derive(Default)]
pub(crate) struct ContainerOptions {
    pub(crate) settle_eps: Option<Expr>,
    pub(crate) before_lerp: Option<ExprPath>,
    pub(crate) coerce: Option<ExprPath>,
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
    pub(crate) fn of(input: &DeriveInput) -> syn::Result<Self> {
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
pub(crate) enum FieldKind {
    /// Forwarded to the field's own `Lerp` and `Animatable` impls.
    Animated,
    /// `#[animate(lerp_only)]`: blends in `lerp`, takes the target at once in motion.
    LerpOnly,
    /// `#[animate(snap)]`: switches halfway through `lerp`, takes the target at once in motion.
    Snap,
}

/// A field's `#[animate(...)]` options.
pub(crate) struct FieldOptions {
    pub(crate) kind: FieldKind,
    pub(crate) settle_eps: Option<Expr>,
}

impl FieldOptions {
    /// Each field's options, in field order; every field's error, combined, where any errs.
    pub(crate) fn of_each(fields: &Punctuated<Field, Comma>) -> syn::Result<Vec<Self>> {
        let mut errors = Errors::default();
        let options = fields
            .iter()
            .filter_map(|field| errors.keep(Self::of(field)))
            .collect();
        errors.or(options)
    }

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

/// Every error of a derive's input, combined, so one compile reports each; syn's
/// [`syn::Error::combine`] joins them, as serde's derive collects its own.
#[derive(Default)]
pub(crate) struct Errors(Option<syn::Error>);

impl Errors {
    /// `result`'s value, or `None` with its error kept.
    pub(crate) fn keep<T>(&mut self, result: syn::Result<T>) -> Option<T> {
        result.map_err(|error| self.push(error)).ok()
    }

    /// Keep `error` beside those kept before.
    pub(crate) fn push(&mut self, error: syn::Error) {
        match &mut self.0 {
            Some(errors) => errors.combine(error),
            None => self.0 = Some(error),
        }
    }

    /// `value` where no error was kept, else every error kept.
    pub(crate) fn or<T>(self, value: T) -> syn::Result<T> {
        self.0.map_or(Ok(value), Err)
    }
}
