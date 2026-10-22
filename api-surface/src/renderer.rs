//! [`Renderer`]: one build's rustdoc JSON rendered into item blocks, keyed by export path.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rustdoc_types::{
    Attribute, Crate, Enum, Generics, Id, Impl, Item, ItemEnum, MacroKind, Module, Path,
    StructKind, Trait, Variant, VariantKind, Visibility,
};

use crate::decl;

/// Traits the compiler implements, or a derive implements beside the trait it names; they say
/// nothing about the crate's API. The std blanket impls (`From<T> for T`, `Into`, ..) drop by
/// their blanket flag, not by name, so a real `impl<T: Num> From<T> for Size` stays.
const NOISE_TRAITS: [&str; 8] = [
    "Send",
    "Sync",
    "Unpin",
    "UnwindSafe",
    "RefUnwindSafe",
    "Freeze",
    "StructuralPartialEq",
    "TrivialClone",
];

/// What one build exports: a block of declaration lines per export path, the names `prelude`
/// re-exports, and the public items no path reaches.
#[derive(Debug)]
pub(crate) struct BuildSurface {
    pub(crate) blocks: BTreeMap<String, Vec<String>>,
    pub(crate) prelude: BTreeSet<String>,
    pub(crate) unreached: BTreeSet<String>,
}

/// What one impl adds to its owner's block.
#[derive(Debug)]
enum ImplShown {
    /// An inherent impl: its public members.
    Members,
    /// A trait impl that says nothing about the API.
    Hidden,
    /// A plain trait impl, named in the declaration's trait list.
    Trait(String),
    /// A trait impl the name alone does not state — generic, bounded, or for another self
    /// type — on a line of its own.
    TraitImpl(String),
}

/// What a type's impls add to its block: the traits its declaration names, and the lines of its
/// inherent members and of the trait impls that need their own.
#[derive(Debug, Default)]
struct Members {
    traits: BTreeSet<String>,
    lines: Vec<String>,
}

impl Members {
    /// `: Clone, Debug, ..` after a declaration's name and generics, or nothing.
    fn trait_list(&self) -> String {
        if self.traits.is_empty() {
            String::new()
        } else {
            format!(
                ": {}",
                self.traits.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        }
    }
}

/// Walks a crate's exports from its root, rendering each item it reaches.
#[derive(Debug)]
pub(crate) struct Renderer<'a> {
    krate: &'a Crate,
    blocks: BTreeMap<String, Vec<String>>,
    prelude: BTreeSet<String>,
    reached: HashSet<Id>,
}

impl<'a> Renderer<'a> {
    pub(crate) fn render(krate: &'a Crate) -> BuildSurface {
        let mut renderer = Self {
            krate,
            blocks: BTreeMap::new(),
            prelude: BTreeSet::new(),
            reached: HashSet::new(),
        };
        let root = renderer
            .item(krate.root)
            .expect("the root module is in the index");
        renderer.walk(root, "");
        let unreached = renderer.unreached();
        BuildSurface {
            blocks: renderer.blocks,
            prelude: renderer.prelude,
            unreached,
        }
    }

    fn item(&self, id: Id) -> Option<&'a Item> {
        self.krate.index.get(&id)
    }

    fn walk(&mut self, module: &'a Item, prefix: &str) {
        let ItemEnum::Module(Module { items, .. }) = &module.inner else {
            return;
        };
        for &id in items {
            if let Some(item) = self
                .item(id)
                .filter(|item| item.visibility == Visibility::Public)
            {
                self.emit(item, prefix, None);
            }
        }
    }

    /// Renders `item` at `prefix` + its name, or `name` when a re-export renames it.
    fn emit(&mut self, item: &'a Item, prefix: &str, name: Option<&str>) {
        if let ItemEnum::Use(use_) = &item.inner {
            let path = format!("{prefix}{}", use_.name);
            match use_.id.and_then(|id| self.item(id)) {
                None => self.add(
                    path,
                    vec![format!("pub use {} as {}", use_.source, use_.name)],
                ),
                Some(target) if use_.is_glob && matches!(target.inner, ItemEnum::Module(_)) => {
                    self.reached.insert(target.id);
                    self.walk(target, prefix);
                }
                Some(target) => self.emit(target, prefix, Some(&use_.name)),
            }
            return;
        }
        self.reached.insert(item.id);
        let name = name.or(item.name.as_deref()).unwrap_or("_");
        let path = format!("{prefix}{name}");
        let mut lines: Vec<String> = item
            .attrs
            .contains(&Attribute::NonExhaustive)
            .then(|| "#[non_exhaustive]".to_owned())
            .into_iter()
            .collect();
        match &item.inner {
            ItemEnum::Module(_) => {
                if name == "prelude" {
                    self.collect_prelude(item);
                } else {
                    self.add(path.clone(), vec!["mod".to_owned()]);
                    self.walk(item, &format!("{path}::"));
                }
                return;
            }
            ItemEnum::Function(f) => lines.push(decl::signature(name, f)),
            ItemEnum::Struct(s) => {
                let g = &s.generics;
                let members = self.members(name, g, &s.impls);
                lines.push(format!(
                    "struct {name}{}{}{}",
                    decl::generics(g),
                    members.trait_list(),
                    decl::where_clause(g)
                ));
                lines.extend(self.struct_fields(&s.kind));
                lines.extend(members.lines);
            }
            ItemEnum::Union(u) => {
                let g = &u.generics;
                let members = self.members(name, g, &u.impls);
                lines.push(format!(
                    "union {name}{}{}{}",
                    decl::generics(g),
                    members.trait_list(),
                    decl::where_clause(g)
                ));
                lines.extend(self.field_lines(&u.fields));
                lines.extend(members.lines);
            }
            ItemEnum::Enum(e) => lines.extend(self.enum_lines(name, e)),
            ItemEnum::Trait(t) => lines.extend(self.trait_lines(name, t)),
            ItemEnum::TraitAlias(alias) => lines.push(format!(
                "trait {name}{} = {}",
                decl::generics(&alias.generics),
                decl::bounds(&alias.params)
            )),
            ItemEnum::TypeAlias(alias) => lines.push(format!(
                "type {name}{} = {}",
                decl::generics(&alias.generics),
                decl::ty(&alias.type_)
            )),
            // The value is left out: rustdoc renders it after `cfg` stripping, so a
            // host-dependent const would make the surface differ between machines.
            ItemEnum::Constant { type_, .. } => {
                lines.push(format!("const {name}: {}", decl::ty(type_)));
            }
            ItemEnum::Static(s) => {
                let mutable = if s.is_mutable { "mut " } else { "" };
                lines.push(format!("static {mutable}{name}: {}", decl::ty(&s.type_)));
            }
            ItemEnum::Macro(_) => lines.push(format!("macro {name}!")),
            ItemEnum::ProcMacro(m) => {
                let kind = match m.kind {
                    MacroKind::Bang => "bang",
                    MacroKind::Attr => "attribute",
                    MacroKind::Derive => "derive",
                };
                lines.push(format!("proc macro {kind} {name}"));
            }
            ItemEnum::ExternCrate { .. } => lines.push(format!("extern crate {name}")),
            ItemEnum::ExternType => lines.push(format!("extern type {name}")),
            // Kinds a module never holds itself; named rather than dropped if one ever does.
            ItemEnum::Use(_)
            | ItemEnum::StructField(_)
            | ItemEnum::Variant(_)
            | ItemEnum::Impl(_)
            | ItemEnum::Primitive(_)
            | ItemEnum::AssocConst { .. }
            | ItemEnum::AssocType { .. } => {
                lines.push(format!("{:?} {name}", item.inner.item_kind()));
            }
        }
        self.add(path, lines);
    }

    /// An item and a re-export of it under one name reach one path; the first keeps it.
    fn add(&mut self, path: String, lines: Vec<String>) {
        self.blocks.entry(path).or_insert(lines);
    }

    fn collect_prelude(&mut self, prelude: &'a Item) {
        let ItemEnum::Module(Module { items, .. }) = &prelude.inner else {
            return;
        };
        for &id in items {
            if let Some(ItemEnum::Use(use_)) = self.item(id).map(|item| &item.inner) {
                self.prelude.insert(use_.name.clone());
            }
        }
    }

    /// The public fields among `ids`, a line each; a tuple field is named by its index.
    fn field_lines(&self, ids: &[Id]) -> Vec<String> {
        ids.iter()
            .filter_map(|&id| self.item(id))
            .filter(|field| field.visibility == Visibility::Public)
            .filter_map(|field| match &field.inner {
                ItemEnum::StructField(t) => Some(format!(
                    "    pub {}: {}",
                    field.name.as_deref().unwrap_or("_"),
                    decl::ty(t)
                )),
                _ => None,
            })
            .collect()
    }

    fn struct_fields(&self, kind: &StructKind) -> Vec<String> {
        match kind {
            StructKind::Unit => Vec::new(),
            StructKind::Plain { fields, .. } => self.field_lines(fields),
            // A tuple field is named by its index, which a private field before it still takes.
            StructKind::Tuple(fields) => fields
                .iter()
                .enumerate()
                .filter_map(|(idx, id)| match id.and_then(|id| self.item(id)) {
                    Some(Item {
                        visibility: Visibility::Public,
                        inner: ItemEnum::StructField(t),
                        ..
                    }) => Some(format!("    pub {idx}: {}", decl::ty(t))),
                    _ => None,
                })
                .collect(),
        }
    }

    fn enum_lines(&self, name: &str, e: &Enum) -> Vec<String> {
        let g = &e.generics;
        let members = self.members(name, g, &e.impls);
        let mut lines = vec![format!(
            "enum {name}{}{}{}",
            decl::generics(g),
            members.trait_list(),
            decl::where_clause(g)
        )];
        for &id in &e.variants {
            if let Some(Item {
                name: Some(variant_name),
                inner: ItemEnum::Variant(variant),
                ..
            }) = self.item(id)
            {
                lines.push(format!("    {}", self.variant(variant_name, variant)));
            }
        }
        if e.has_stripped_variants {
            lines.push("    // and private variants".to_owned());
        }
        lines.extend(members.lines);
        lines
    }

    fn field_type(&self, id: Id) -> String {
        match self.item(id).map(|field| &field.inner) {
            Some(ItemEnum::StructField(t)) => decl::ty(t),
            _ => "_".to_owned(),
        }
    }

    fn variant(&self, name: &str, v: &Variant) -> String {
        let discriminant = v
            .discriminant
            .as_ref()
            .map(|d| format!(" = {}", d.expr))
            .unwrap_or_default();
        match &v.kind {
            VariantKind::Plain => format!("{name}{discriminant}"),
            VariantKind::Tuple(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|id| id.map_or_else(|| "_".to_owned(), |id| self.field_type(id)))
                    .collect();
                format!("{name}({}){discriminant}", parts.join(", "))
            }
            VariantKind::Struct {
                fields,
                has_stripped_fields,
            } => {
                let mut parts: Vec<String> = fields
                    .iter()
                    .map(|&id| {
                        let field = self.item(id).and_then(|f| f.name.as_deref()).unwrap_or("_");
                        format!("{field}: {}", self.field_type(id))
                    })
                    .collect();
                if *has_stripped_fields {
                    parts.push("..".to_owned());
                }
                format!("{name} {{ {} }}{discriminant}", parts.join(", "))
            }
        }
    }

    fn trait_lines(&self, name: &str, t: &Trait) -> Vec<String> {
        let g = &t.generics;
        let supers = if t.bounds.is_empty() {
            String::new()
        } else {
            format!(": {}", decl::bounds(&t.bounds))
        };
        let unsafe_ = if t.is_unsafe { "unsafe " } else { "" };
        let auto = if t.is_auto { "auto " } else { "" };
        let mut lines = vec![format!(
            "{unsafe_}{auto}trait {name}{}{supers}{}",
            decl::generics(g),
            decl::where_clause(g)
        )];
        for &id in &t.items {
            if let Some(item) = self.item(id) {
                lines.push(format!("    {}", trait_item(item)));
            }
        }
        // This trait's impls for a bare type parameter: `impl<T: ..> Tr for T`.
        for &id in &t.implementations {
            if let Some(Item {
                inner: ItemEnum::Impl(impl_),
                ..
            }) = self.item(id)
                && matches!(impl_.for_, rustdoc_types::Type::Generic(_))
            {
                lines.push(format!(
                    "    impl{} {name} for {}{}",
                    decl::generics(&impl_.generics),
                    decl::ty(&impl_.for_),
                    decl::where_clause(&impl_.generics)
                ));
            }
        }
        lines
    }

    fn is_local(&self, trait_: &Path) -> bool {
        self.krate
            .paths
            .get(&trait_.id)
            .is_some_and(|summary| summary.crate_id == 0)
    }

    /// The inherent members and trait impls of the type `owner`.
    ///
    /// `impls` also holds impls that only name the type in an argument (`impl From<RgbaF32> for
    /// Brush`), so an impl prints its self type whenever that is not the owner. An inherent impl
    /// on a narrower self type (`impl Gradient<LinearGeometry>`) heads its own members.
    fn members(&self, owner: &str, g: &Generics, impl_ids: &[Id]) -> Members {
        let plain = decl::anon(&format!("{owner}{}", decl::param_names(g)));
        let declared = decl::unbound(&decl::generics(g));
        let mut lines = Vec::new();
        let mut traits = BTreeSet::new();
        let mut trait_impls = BTreeSet::new();
        let mut head_shown: Option<String> = None;
        for &id in impl_ids {
            let Some(Item {
                inner: ItemEnum::Impl(impl_),
                ..
            }) = self.item(id)
            else {
                continue;
            };
            if impl_.is_synthetic {
                continue;
            }
            match self.shown(impl_, &plain, &declared) {
                ImplShown::Members => {}
                ImplShown::Hidden => continue,
                ImplShown::Trait(name) => {
                    traits.insert(name);
                    continue;
                }
                ImplShown::TraitImpl(line) => {
                    trait_impls.insert(line);
                    continue;
                }
            }
            let for_ = decl::ty(&impl_.for_);
            let generics = decl::generics(&impl_.generics);
            let where_ = decl::where_clause(&impl_.generics);
            // Members of an impl that narrows the type or bounds its parameters sit under that
            // impl's head.
            let indent = if decl::anon(&for_) != plain
                || decl::unbound(&generics) != declared
                || !where_.is_empty()
            {
                let head = format!("    impl{generics} {for_}{where_}");
                if head_shown.as_ref() != Some(&head) {
                    lines.push(head.clone());
                    head_shown = Some(head);
                }
                "        "
            } else {
                head_shown = None;
                "    "
            };
            for &member in &impl_.items {
                let Some(m) = self
                    .item(member)
                    .filter(|m| m.visibility == Visibility::Public)
                else {
                    continue;
                };
                let name = m.name.as_deref().unwrap_or("_");
                let line = match &m.inner {
                    ItemEnum::Function(f) => decl::signature(name, f),
                    ItemEnum::AssocConst { type_, .. } => {
                        format!("const {name}: {}", decl::ty(type_))
                    }
                    ItemEnum::AssocType { type_, .. } => format!(
                        "type {name} = {}",
                        type_.as_ref().map(decl::ty).unwrap_or_default()
                    ),
                    other => format!("{:?} {name}", other.item_kind()),
                };
                lines.push(format!("{indent}pub {line}"));
            }
        }
        lines.extend(trait_impls.into_iter().map(|t| format!("    {t}")));
        Members { traits, lines }
    }

    /// What `impl_` adds to the block of the type `plain` names, whose own generics, without
    /// their lifetimes, are `declared`.
    fn shown(&self, impl_: &Impl, plain: &str, declared: &str) -> ImplShown {
        let Some(trait_) = &impl_.trait_ else {
            return ImplShown::Members;
        };
        let name = decl::path_name(trait_);
        if impl_.blanket_impl.is_some() {
            // A std blanket impl (`impl<T> From<T> for T`) says nothing about this type. This
            // crate's own blanket impl does: it is how the type gets `DockTab` or `Lower`, and
            // the trait's block shows the impl itself.
            return if self.is_local(trait_) {
                ImplShown::Trait(format!("{name} (blanket)"))
            } else {
                ImplShown::Hidden
            };
        }
        if NOISE_TRAITS.contains(&decl::last_segment(trait_)) {
            return ImplShown::Hidden;
        }
        let generics = decl::generics(&impl_.generics);
        let where_ = decl::where_clause(&impl_.generics);
        let for_ = decl::ty(&impl_.for_);
        // An impl over the type's own parameters, bound no further, is the type's own.
        let own = decl::unbound(&generics) == declared && decl::anon(&for_) == plain;
        if own && where_.is_empty() && !impl_.is_unsafe && !impl_.is_negative {
            return ImplShown::Trait(name);
        }
        let unsafe_ = if impl_.is_unsafe { "unsafe " } else { "" };
        let negative = if impl_.is_negative { "!" } else { "" };
        let mut head = format!("{unsafe_}impl{generics} {negative}{name}");
        if !own {
            head += " for ";
            head += &for_;
        }
        head += &where_;
        ImplShown::TraitImpl(head)
    }

    /// Public items defined in this crate that no export path reaches. Members of an impl or a
    /// trait are reached with their owner, so they are not counted on their own.
    fn unreached(&self) -> BTreeSet<String> {
        let members: HashSet<Id> = self
            .krate
            .index
            .values()
            .flat_map(|item| match &item.inner {
                ItemEnum::Impl(impl_) => impl_.items.as_slice(),
                ItemEnum::Trait(trait_) => trait_.items.as_slice(),
                _ => &[],
            })
            .copied()
            .collect();
        self.krate
            .index
            .iter()
            .filter(|(id, item)| {
                !members.contains(id)
                    && !self.reached.contains(id)
                    && item.crate_id == 0
                    && item.visibility == Visibility::Public
            })
            .filter_map(|(id, item)| {
                let kind = exported_kind(&item.inner)?;
                let path = self.krate.paths.get(id).map_or_else(
                    || item.name.clone().unwrap_or_else(|| format!("{id:?}")),
                    |summary| summary.path.join("::"),
                );
                // A sealed supertrait is unnameable by design: the public marker over it is
                // the API.
                (!path.contains("::sealed::")).then(|| format!("{kind:<10} {path}"))
            })
            .collect()
    }
}

/// A trait's own item: a method with its signature, an associated type or constant.
fn trait_item(item: &Item) -> String {
    let name = item.name.as_deref().unwrap_or("_");
    match &item.inner {
        ItemEnum::Function(f) => {
            let body = if f.has_body { " { .. }" } else { "" };
            format!("{}{body}", decl::signature(name, f))
        }
        ItemEnum::AssocType {
            generics,
            bounds,
            type_,
            ..
        } => {
            let bounds = if bounds.is_empty() {
                String::new()
            } else {
                format!(": {}", decl::bounds(bounds))
            };
            let default = type_.as_ref().map(|t| format!(" = {}", decl::ty(t)));
            format!(
                "type {name}{}{bounds}{}",
                decl::generics(generics),
                default.unwrap_or_default()
            )
        }
        ItemEnum::AssocConst { type_, value, .. } => {
            let default = value.as_ref().map(|v| format!(" = {v}"));
            format!(
                "const {name}: {}{}",
                decl::ty(type_),
                default.unwrap_or_default()
            )
        }
        other => format!("{:?} {name}", other.item_kind()),
    }
}

/// The kind an export path can name, for an item that should be reachable by one; `None` for a
/// module, a re-export, and the parts of other items.
const fn exported_kind(inner: &ItemEnum) -> Option<&'static str> {
    match inner {
        ItemEnum::Struct(_) => Some("struct"),
        ItemEnum::Enum(_) => Some("enum"),
        ItemEnum::Union(_) => Some("union"),
        ItemEnum::Trait(_) => Some("trait"),
        ItemEnum::TraitAlias(_) => Some("trait_alias"),
        ItemEnum::Function(_) => Some("function"),
        ItemEnum::TypeAlias(_) => Some("type_alias"),
        ItemEnum::Constant { .. } => Some("constant"),
        ItemEnum::Static(_) => Some("static"),
        ItemEnum::Macro(_) => Some("macro"),
        ItemEnum::ProcMacro(_) => Some("proc_macro"),
        ItemEnum::Module(_)
        | ItemEnum::Use(_)
        | ItemEnum::ExternCrate { .. }
        | ItemEnum::StructField(_)
        | ItemEnum::Variant(_)
        | ItemEnum::Impl(_)
        | ItemEnum::ExternType
        | ItemEnum::Primitive(_)
        | ItemEnum::AssocConst { .. }
        | ItemEnum::AssocType { .. } => None,
    }
}
