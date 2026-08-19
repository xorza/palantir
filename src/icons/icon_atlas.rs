use crate::icons::svg_facts::SvgFacts;
use crate::primitives::span::Span;
use glam::Vec2;
use std::borrow::Cow;

/// Index of one icon within its [`IconAtlas`]. A generated set emits a named
/// constant per icon, so a call site says `icons::SAVE` rather than a number;
/// a runtime-built one resolves through [`IconSet::by_name`](crate::IconSet::by_name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconId(pub u16);

/// One baked icon: what it is called, the size it was drawn at, where its
/// normalized SVG sits in the set's blob, and the two facts the renderer needs
/// before it has parsed anything.
#[derive(Clone, Copy, Debug)]
pub struct IconDef {
    /// Lowercase kebab-case, derived from the source filename. Unique within
    /// a set, and the key [`IconSet::by_name`](crate::IconSet::by_name)
    /// searches.
    pub name: &'static str,
    /// The SVG's viewBox extent in logical px — the size the artwork was
    /// designed at, and what to size a node to.
    pub view_box: Vec2,
    /// Byte range of this icon's SVG within its set's concatenated blob.
    pub svg: Span,
    /// Every paint in the icon resolves to one colour (or `currentColor`), so
    /// it rasterizes to a coverage mask and takes the shape's full tint. A
    /// colour icon rasterizes to RGBA instead, where tint modulates alpha
    /// alone.
    pub tintable: bool,
    /// The icon uses an SVG filter. Measured at 10-20x the raster cost of one
    /// without, growing superlinearly with size, which is why these are
    /// prewarmed at load rather than rasterized on the frame that first draws
    /// them.
    pub filtered: bool,
}

/// An icon set: a name-sorted table plus every icon's SVG in one blob.
///
/// Two ways in, and the difference is only who owns the bytes. [`Self::baked`]
/// borrows data compiled into the binary — a generated `const` — and copies
/// nothing; [`Self::from_svgs`] builds one at runtime and owns its buffers.
/// Both are held behind an `Rc` once loaded, so a set is shared rather than
/// duplicated and nothing has to outlive the app to stay reachable.
///
/// Rasterization parses lazily either way — per icon, the first time that icon
/// is drawn — so a set the session never draws from rasterizes nothing. What
/// differs is the *table*: [`Self::baked`] arrives with one already built and
/// so parses nothing at construction, while [`Self::from_svgs`] has to read
/// each source to fill it.
///
/// The table is sorted by [`IconDef::name`], which is what lets
/// [`IconSet::by_name`](crate::IconSet::by_name) binary-search it.
#[derive(Debug)]
pub struct IconAtlas {
    icons: Cow<'static, [IconDef]>,
    /// Every icon's SVG, concatenated. Sliced by [`IconDef::svg`].
    svg: Cow<'static, [u8]>,
}

impl IconAtlas {
    /// A set from data compiled into the binary — what a generated
    /// `icons.rs` calls. Borrows both halves, so it allocates nothing and
    /// parses nothing.
    ///
    /// `icons` must be sorted by [`IconDef::name`] and each entry's
    /// [`IconDef::svg`] must span its own slice of `svg` — the two invariants
    /// name lookup and per-icon slicing rest on. Whatever generates the table
    /// owes them; [`Self::from_svgs`] establishes them itself.
    pub const fn baked(icons: &'static [IconDef], svg: &'static [u8]) -> Self {
        Self {
            icons: Cow::Borrowed(icons),
            svg: Cow::Borrowed(svg),
        }
    }

    /// Build a set from SVG sources at runtime, deriving each icon's viewBox,
    /// tintability, and filter use from one parse of it.
    ///
    /// Owns its buffers, so nothing leaks and the set is freed with the last
    /// [`IconSet`](crate::IconSet) holding it. It does pay a parse per icon,
    /// which is why a set that ships with the app should come from
    /// [`Self::baked`] instead.
    ///
    /// **That parse is paid twice for any icon the session goes on to draw**:
    /// the tree read here is dropped once its three facts are off it, and
    /// `IconRasterizer` parses the same bytes again to get pixels. Keeping this
    /// one instead would mean an [`IconAtlas`] that holds `usvg::Tree`s — a
    /// parser type in a data type, and one retained tree per icon in the set
    /// rather than per icon actually drawn. At ~180 µs a parse, once, on the
    /// path that already tells you to prefer [`Self::baked`], the memory and
    /// the coupling cost more than the microseconds do.
    ///
    /// Entries are sorted by name, which is the order a baked set guarantees
    /// and [`IconSet::by_name`](crate::IconSet::by_name) binary-searches — so
    /// resolve ids by name rather than by the order they were passed in.
    ///
    /// An unparseable source is skipped; it would have failed to rasterize
    /// anyway, and dropping it here keeps one broken icon from taking the set
    /// with it.
    pub fn from_svgs<'a>(sources: impl IntoIterator<Item = (&'static str, &'a str)>) -> Self {
        let mut surveyed: Vec<(&'static str, &'a str, SvgFacts)> = sources
            .into_iter()
            .filter_map(|(name, svg)| Some((name, svg, SvgFacts::of(svg.as_bytes())?)))
            .collect();
        surveyed.sort_unstable_by_key(|(name, ..)| *name);

        let mut blob: Vec<u8> = Vec::with_capacity(surveyed.iter().map(|(_, s, _)| s.len()).sum());
        let mut icons: Vec<IconDef> = Vec::with_capacity(surveyed.len());
        for &(name, svg, facts) in &surveyed {
            let start = blob.len() as u32;
            blob.extend_from_slice(svg.as_bytes());
            icons.push(IconDef {
                name,
                view_box: facts.view_box,
                svg: Span::new(start, svg.len() as u32),
                tintable: facts.tintable,
                filtered: facts.filtered,
            });
        }
        Self {
            icons: Cow::Owned(icons),
            svg: Cow::Owned(blob),
        }
    }

    /// The name-sorted table, for the two places that walk the whole set —
    /// the prewarm pass and `by_name`. Everything else goes through an
    /// [`IconId`].
    pub(crate) fn icons(&self) -> &[IconDef] {
        &self.icons
    }

    /// The definition behind `icon`.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set. Ids come from the generated
    /// constants or from [`IconSet::by_name`](crate::IconSet::by_name), so an
    /// out-of-range one means an id crossed between sets.
    pub(crate) fn def(&self, icon: IconId) -> &IconDef {
        let icons = &self.icons;
        assert!(
            (icon.0 as usize) < icons.len(),
            "IconId({}) is not in this set ({} icons) — an id from another set?",
            icon.0,
            icons.len(),
        );
        &icons[icon.0 as usize]
    }

    /// This icon's normalized SVG bytes.
    pub(crate) fn svg_bytes(&self, icon: IconId) -> &[u8] {
        &self.svg[self.def(icon).svg.range()]
    }
}

#[cfg(test)]
mod tests {
    use crate::icons::icon_atlas::{IconAtlas, IconId};
    use glam::Vec2;

    const ONE_COLOUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><rect width="24" height="12" fill="#4080c0"/><circle cx="6" cy="6" r="3" fill="#4080c0"/></svg>"##;
    const TWO_COLOURS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#f00"/><rect x="8" width="8" height="16" fill="#00f"/></svg>"##;

    /// The set-level invariants `from_svgs` guarantees: name order, which
    /// `by_name`'s binary search rests on. What each icon is classified *as*
    /// is `SvgFacts`' own test.
    #[test]
    fn runtime_build_sorts_by_name() {
        let atlas = IconAtlas::from_svgs([("two", TWO_COLOURS), ("one", ONE_COLOUR)]);
        let names: Vec<&str> = atlas.icons().iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            ["one", "two"],
            "sorted, whatever order they arrived in"
        );
        assert_eq!(atlas.icons()[0].view_box, Vec2::new(24.0, 12.0));
        assert!(atlas.icons()[0].tintable, "facts ride through to the def");
    }

    /// Each icon's span must slice its own source back out of the shared blob
    /// — an off-by-one here would rasterize the neighbouring icon.
    #[test]
    fn spans_slice_each_icon_out_of_the_shared_blob() {
        let atlas = IconAtlas::from_svgs([("b", TWO_COLOURS), ("a", ONE_COLOUR)]);
        assert_eq!(atlas.svg_bytes(IconId(0)), ONE_COLOUR.as_bytes());
        assert_eq!(atlas.svg_bytes(IconId(1)), TWO_COLOURS.as_bytes());
    }

    /// One broken source must not take the set with it.
    #[test]
    fn unparseable_sources_are_skipped() {
        let atlas = IconAtlas::from_svgs([("good", ONE_COLOUR), ("bad", "<svg")]);
        assert_eq!(atlas.icons().len(), 1);
        assert_eq!(atlas.icons()[0].name, "good");
        assert_eq!(
            atlas.svg_bytes(IconId(0)),
            ONE_COLOUR.as_bytes(),
            "the blob holds only what parsed",
        );
    }
}
