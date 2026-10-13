//! Rust source text for the pieces of a declaration: types, bounds, generics and signatures.
//!
//! A path renders as its last segment, as the declaration names it; a type is identified by its
//! item block, not by where its crate keeps it.

use rustdoc_types::{
    Abi, AssocItemConstraint, AssocItemConstraintKind, Function, GenericArg, GenericArgs,
    GenericBound, GenericParamDef, GenericParamDefKind, Generics, Path, PolyTrait,
    PreciseCapturingArg, Term, TraitBoundModifier, Type, WherePredicate,
};

pub(crate) fn ty(t: &Type) -> String {
    match t {
        Type::ResolvedPath(path) => path_name(path),
        Type::Generic(name) | Type::Primitive(name) => name.clone(),
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let lifetime = lifetime
                .as_ref()
                .map(|l| format!("{l} "))
                .unwrap_or_default();
            let mutable = if *is_mutable { "mut " } else { "" };
            format!("&{lifetime}{mutable}{}", ty(type_))
        }
        Type::RawPointer { is_mutable, type_ } => {
            let mutable = if *is_mutable { "mut" } else { "const" };
            format!("*{mutable} {}", ty(type_))
        }
        Type::Tuple(types) => match types.as_slice() {
            [one] => format!("({},)", ty(one)),
            many => format!("({})", list(many.iter().map(ty))),
        },
        Type::Slice(type_) => format!("[{}]", ty(type_)),
        Type::Array { type_, len } => format!("[{}; {len}]", ty(type_)),
        Type::Pat { type_, .. } => ty(type_),
        Type::ImplTrait(bounds_) => format!("impl {}", bounds(bounds_)),
        Type::DynTrait(dyn_trait) => {
            let mut parts: Vec<String> = dyn_trait.traits.iter().map(poly).collect();
            parts.extend(dyn_trait.lifetime.clone());
            format!("dyn {}", parts.join(" + "))
        }
        Type::FunctionPointer(pointer) => {
            let hrtb = for_params(&pointer.generic_params);
            let inputs = list(pointer.sig.inputs.iter().map(|(_, t)| ty(t)));
            let output = pointer
                .sig
                .output
                .as_ref()
                .map(|o| format!(" -> {}", ty(o)));
            format!(
                "{hrtb}{}fn({inputs}){}",
                header(
                    pointer.header.is_const,
                    pointer.header.is_async,
                    pointer.header.is_unsafe,
                    &pointer.header.abi
                ),
                output.unwrap_or_default()
            )
        }
        Type::QualifiedPath {
            name,
            args: assoc_args,
            self_type,
            trait_,
        } => {
            let assoc = assoc_args.as_deref().map(args).unwrap_or_default();
            // `Self::Tab` comes with the trait, but an empty path.
            match trait_ {
                Some(trait_) if !trait_.path.is_empty() => {
                    format!(
                        "<{} as {}>::{name}{assoc}",
                        ty(self_type),
                        path_name(trait_)
                    )
                }
                _ => format!("{}::{name}{assoc}", ty(self_type)),
            }
        }
        Type::Infer => "_".to_owned(),
    }
}

/// A path's last segment and its generic arguments: `Rc<Theme>`, `Into<Size>`.
pub(crate) fn path_name(path: &Path) -> String {
    let name = path.path.rsplit("::").next().unwrap_or(&path.path);
    format!(
        "{name}{}",
        path.args.as_deref().map(args).unwrap_or_default()
    )
}

/// The last segment of a path, without its arguments.
pub(crate) fn last_segment(path: &Path) -> &str {
    path.path.rsplit("::").next().unwrap_or(&path.path)
}

pub(crate) fn args(a: &GenericArgs) -> String {
    match a {
        GenericArgs::AngleBracketed { args, constraints } => {
            let parts: Vec<String> = args
                .iter()
                .map(|arg| match arg {
                    GenericArg::Lifetime(lifetime) => lifetime.clone(),
                    GenericArg::Type(t) => ty(t),
                    GenericArg::Const(constant) => constant.expr.clone(),
                    GenericArg::Infer => "_".to_owned(),
                })
                .chain(constraints.iter().map(constraint))
                .collect();
            if parts.is_empty() {
                String::new()
            } else {
                format!("<{}>", parts.join(", "))
            }
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let output = output.as_ref().map(|o| format!(" -> {}", ty(o)));
            format!(
                "({}){}",
                list(inputs.iter().map(ty)),
                output.unwrap_or_default()
            )
        }
        GenericArgs::ReturnTypeNotation => "(..)".to_owned(),
    }
}

fn constraint(c: &AssocItemConstraint) -> String {
    let name = format!(
        "{}{}",
        c.name,
        c.args.as_deref().map(args).unwrap_or_default()
    );
    match &c.binding {
        AssocItemConstraintKind::Equality(value) => format!("{name} = {}", term(value)),
        AssocItemConstraintKind::Constraint(bounds_) => format!("{name}: {}", bounds(bounds_)),
    }
}

fn term(t: &Term) -> String {
    match t {
        Term::Type(t) => ty(t),
        Term::Constant(constant) => constant.expr.clone(),
    }
}

fn poly(p: &PolyTrait) -> String {
    format!("{}{}", for_params(&p.generic_params), path_name(&p.trait_))
}

/// A higher-ranked binder, `for<'a> `, or nothing.
fn for_params(params: &[GenericParamDef]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("for<{}> ", list(params.iter().map(|p| p.name.clone())))
    }
}

pub(crate) fn bounds(bounds: &[GenericBound]) -> String {
    let parts: Vec<String> = bounds
        .iter()
        .map(|bound| match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => {
                let modifier = match modifier {
                    TraitBoundModifier::None => "",
                    TraitBoundModifier::Maybe => "?",
                    TraitBoundModifier::MaybeConst => "[const] ",
                };
                format!(
                    "{}{modifier}{}",
                    for_params(generic_params),
                    path_name(trait_)
                )
            }
            GenericBound::Outlives(lifetime) => lifetime.clone(),
            GenericBound::Use(captured) => format!(
                "use<{}>",
                list(captured.iter().map(|arg| match arg {
                    PreciseCapturingArg::Lifetime(name) | PreciseCapturingArg::Param(name) => {
                        name.clone()
                    }
                }))
            ),
        })
        .collect();
    parts.join(" + ")
}

/// `<'a, T: Bound = Default, const N: usize>`, or nothing; an `impl Trait` argument's
/// synthetic parameter shows at the argument instead.
pub(crate) fn generics(g: &Generics) -> String {
    let params: Vec<String> = g
        .params
        .iter()
        .filter_map(|p| match &p.kind {
            GenericParamDefKind::Lifetime { outlives } => Some(if outlives.is_empty() {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, outlives.join(" + "))
            }),
            GenericParamDefKind::Type {
                is_synthetic: true, ..
            } => None,
            GenericParamDefKind::Type {
                bounds: bounds_,
                default,
                is_synthetic: false,
            } => {
                let mut s = p.name.clone();
                if !bounds_.is_empty() {
                    s += ": ";
                    s += &bounds(bounds_);
                }
                if let Some(default) = default {
                    s += " = ";
                    s += &ty(default);
                }
                Some(s)
            }
            GenericParamDefKind::Const { type_, default } => {
                let default = default.as_ref().map(|d| format!(" = {d}"));
                Some(format!(
                    "const {}: {}{}",
                    p.name,
                    ty(type_),
                    default.unwrap_or_default()
                ))
            }
        })
        .collect();
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}

/// The names a type's own generics bind, as its self type spells them: `<'a, T>`.
pub(crate) fn param_names(g: &Generics) -> String {
    let names: Vec<&str> = g
        .params
        .iter()
        .filter(|p| {
            !matches!(
                p.kind,
                GenericParamDefKind::Type {
                    is_synthetic: true,
                    ..
                }
            )
        })
        .map(|p| p.name.as_str())
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("<{}>", names.join(", "))
    }
}

/// ` where T: Bound, 'a: 'b`, or nothing.
pub(crate) fn where_clause(g: &Generics) -> String {
    let predicates: Vec<String> = g
        .where_predicates
        .iter()
        .map(|w| match w {
            WherePredicate::BoundPredicate {
                type_,
                bounds: bounds_,
                generic_params,
            } => format!(
                "{}{}: {}",
                for_params(generic_params),
                ty(type_),
                bounds(bounds_)
            ),
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                format!("{lifetime}: {}", outlives.join(" + "))
            }
            WherePredicate::EqPredicate { lhs, rhs } => format!("{} = {}", ty(lhs), term(rhs)),
        })
        .collect();
    if predicates.is_empty() {
        String::new()
    } else {
        format!(" where {}", predicates.join(", "))
    }
}

/// `const unsafe fn name<T>(&self, x: T) -> R where ..`.
pub(crate) fn signature(name: &str, f: &Function) -> String {
    let mut inputs: Vec<String> = f
        .sig
        .inputs
        .iter()
        .map(|(param, t)| {
            let shown = ty(t);
            if param == "self" {
                receiver(&shown).unwrap_or_else(|| format!("self: {shown}"))
            } else {
                format!("{param}: {shown}")
            }
        })
        .collect();
    if f.sig.is_c_variadic {
        inputs.push("...".to_owned());
    }
    let output = f.sig.output.as_ref().map(|o| format!(" -> {}", ty(o)));
    format!(
        "{}fn {name}{}({}){}{}",
        header(
            f.header.is_const,
            f.header.is_async,
            f.header.is_unsafe,
            &f.header.abi
        ),
        generics(&f.generics),
        inputs.join(", "),
        output.unwrap_or_default(),
        where_clause(&f.generics)
    )
}

/// `const async unsafe extern "C" `, each word only where it applies.
fn header(is_const: bool, is_async: bool, is_unsafe: bool, abi: &Abi) -> String {
    let mut out = String::new();
    for (word, on) in [
        ("const ", is_const),
        ("async ", is_async),
        ("unsafe ", is_unsafe),
    ] {
        if on {
            out += word;
        }
    }
    let abi = match abi {
        Abi::Rust => None,
        Abi::C { unwind } => Some(("C", *unwind)),
        Abi::Cdecl { unwind } => Some(("cdecl", *unwind)),
        Abi::Stdcall { unwind } => Some(("stdcall", *unwind)),
        Abi::Fastcall { unwind } => Some(("fastcall", *unwind)),
        Abi::Aapcs { unwind } => Some(("aapcs", *unwind)),
        Abi::Win64 { unwind } => Some(("win64", *unwind)),
        Abi::SysV64 { unwind } => Some(("sysv64", *unwind)),
        Abi::System { unwind } => Some(("system", *unwind)),
        Abi::Other(name) => Some((name.as_str(), false)),
    };
    if let Some((name, unwind)) = abi {
        let unwind = if unwind { "-unwind" } else { "" };
        out += "extern \"";
        out += name;
        out += unwind;
        out += "\" ";
    }
    out
}

/// A receiver's shorthand — `self`, `&self`, `&'a mut self` — for a self type that has one.
fn receiver(shown: &str) -> Option<String> {
    let rest = shown.strip_suffix("Self")?;
    let shorthand = rest.is_empty()
        || rest.strip_prefix('&').is_some_and(|after| {
            let after = match after.strip_prefix('\'') {
                Some(lifetime) => match lifetime.split_once(' ') {
                    Some((name, after)) if !name.is_empty() && name.chars().all(is_word) => after,
                    _ => return false,
                },
                None => after,
            };
            after.is_empty() || after == "mut "
        });
    shorthand.then(|| format!("{rest}self"))
}

/// `text` with every lifetime named `'_`, so `Button<'a>` matches `Button<'_>`.
pub(crate) fn anon(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\'' && chars.peek().is_some_and(|&n| is_word(n)) {
            while chars.next_if(|&n| is_word(n)).is_some() {}
            out.push('_');
        }
    }
    out
}

/// A generics list without its lifetimes: `impl Menu<'_>` elides what `struct Menu<'a>`
/// declares, and narrows nothing.
pub(crate) fn unbound(generics: &str) -> String {
    anon(generics)
        .replace("'_, ", "")
        .replace("'_", "")
        .replace("<>", "")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn list(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use crate::decl::{anon, receiver, unbound};

    /// Every receiver shorthand, and the self types that have none.
    #[test]
    fn a_receiver_takes_its_shorthand() {
        assert_eq!(receiver("Self").as_deref(), Some("self"));
        assert_eq!(receiver("&Self").as_deref(), Some("&self"));
        assert_eq!(receiver("&mut Self").as_deref(), Some("&mut self"));
        assert_eq!(receiver("&'a Self").as_deref(), Some("&'a self"));
        assert_eq!(receiver("&'a mut Self").as_deref(), Some("&'a mut self"));
        assert_eq!(receiver("Rc<Self>"), None);
        assert_eq!(receiver("Box<Self>"), None);
        assert_eq!(receiver("&' Self"), None);
    }

    /// Lifetimes read as `'_`, and a generics list drops them whole.
    #[test]
    fn lifetimes_are_anonymous_and_unbound_drops_them() {
        assert_eq!(anon("Button<'a, T>"), "Button<'_, T>");
        assert_eq!(anon("&'static str"), "&'_ str");
        assert_eq!(unbound("<'a, T>"), "<T>");
        assert_eq!(unbound("<'a>"), "");
    }
}
