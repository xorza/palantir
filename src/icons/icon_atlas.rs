use crate::primitives::span::Span;
use glam::Vec2;
use resvg::usvg;
use std::borrow::Cow;

/// Index of one icon within its [`IconAtlas`]. `bake-icons` emits a named
/// constant per icon, so a call site says `icons::SAVE` rather than a number.
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
    /// Byte range of this icon's normalized SVG in [`IconAtlas::svg`].
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
/// borrows data compiled into the binary — `bake-icons` output — and copies
/// nothing; [`Self::from_svgs`] builds one at runtime and owns its buffers.
/// Both are held behind an `Rc` once loaded, so a set is shared rather than
/// duplicated and nothing has to outlive the app to stay reachable.
///
/// Either way the SVG is parsed lazily, per icon, the first time that icon is
/// rasterized — a set the session never draws from costs nothing beyond its
/// bytes.
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
    /// [`IconDef::svg`] must span its own slice of `svg`; `bake-icons`
    /// guarantees both.
    pub const fn baked(icons: &'static [IconDef], svg: &'static [u8]) -> Self {
        Self {
            icons: Cow::Borrowed(icons),
            svg: Cow::Borrowed(svg),
        }
    }

    /// Build a set from SVG sources at runtime, deriving each icon's viewBox,
    /// tintability, and filter use by parsing it — the same classification
    /// `bake-icons` does, run at startup instead of at build time.
    ///
    /// Owns its buffers, so nothing leaks and the set is freed with the last
    /// [`IconSet`](crate::IconSet) holding it. It does pay a parse per icon,
    /// which is why a set that ships with the app should come from
    /// [`Self::baked`] instead.
    ///
    /// Entries are sorted by name, which is the order a baked set guarantees
    /// and [`IconSet::by_name`](crate::IconSet::by_name) binary-searches — so
    /// resolve ids by name rather than by the order they were passed in.
    ///
    /// An unparseable source is skipped; it would have failed to rasterize
    /// anyway, and dropping it here keeps one broken icon from taking the set
    /// with it.
    pub fn from_svgs<'a>(sources: impl IntoIterator<Item = (&'static str, &'a str)>) -> Self {
        let options = usvg::Options::default();
        let mut parsed: Vec<(&'static str, &'a str, usvg::Tree)> = sources
            .into_iter()
            .filter_map(|(name, svg)| {
                let tree = usvg::Tree::from_data(svg.as_bytes(), &options).ok()?;
                Some((name, svg, tree))
            })
            .collect();
        parsed.sort_unstable_by_key(|(name, ..)| *name);

        let mut blob: Vec<u8> = Vec::with_capacity(parsed.iter().map(|(_, s, _)| s.len()).sum());
        let mut icons: Vec<IconDef> = Vec::with_capacity(parsed.len());
        for (name, svg, tree) in &parsed {
            let start = blob.len() as u32;
            blob.extend_from_slice(svg.as_bytes());
            let paint = PaintSurvey::of(tree);
            icons.push(IconDef {
                name,
                view_box: Vec2::new(tree.size().width(), tree.size().height()),
                svg: Span::new(start, svg.len() as u32),
                tintable: paint.tintable,
                filtered: paint.filtered,
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

/// What one walk of a parsed SVG found about how it paints — the two facts an
/// [`IconDef`] carries that cannot be read off the markup without resolving it.
///
/// `bake-icons` will run this same survey at build time; it lives here so the
/// build-time and runtime paths classify identically rather than drifting into
/// two answers for one icon.
#[derive(Debug)]
struct PaintSurvey {
    /// Every paint resolved to at most one colour, so the artwork carries no
    /// colour of its own worth keeping and the draw's tint can supply it.
    tintable: bool,
    /// Some group carries an SVG filter — 10-20x the raster cost, which is
    /// what the renderer prewarms against.
    filtered: bool,
    /// The one colour seen so far, if the walk has seen exactly one. Walk
    /// state rather than a result: a second distinct colour clears
    /// [`Self::tintable`] and this stops mattering.
    only: Option<usvg::Color>,
}

impl PaintSurvey {
    fn of(tree: &usvg::Tree) -> Self {
        let mut survey = Self {
            tintable: true,
            filtered: false,
            only: None,
        };
        survey.walk(tree.root());
        survey
    }

    fn walk(&mut self, group: &usvg::Group) {
        if !group.filters().is_empty() {
            self.filtered = true;
        }
        for node in group.children() {
            match node {
                usvg::Node::Group(child) => self.walk(child),
                usvg::Node::Path(path) => {
                    let paints = [
                        path.fill().map(usvg::Fill::paint),
                        path.stroke().map(usvg::Stroke::paint),
                    ];
                    for paint in paints.into_iter().flatten() {
                        self.note(paint);
                    }
                }
                // A raster image or unresolved text carries colour this walk
                // cannot account for, so neither can be tinted away.
                usvg::Node::Image(_) | usvg::Node::Text(_) => self.tintable = false,
            }
        }
    }

    /// Fold one paint into the survey: a second distinct colour, or any
    /// gradient or pattern, means the artwork's own colours have to survive.
    fn note(&mut self, paint: &usvg::Paint) {
        match paint {
            usvg::Paint::Color(color) => match self.only {
                Some(seen) if seen != *color => self.tintable = false,
                Some(_) => {}
                None => self.only = Some(*color),
            },
            usvg::Paint::LinearGradient(_)
            | usvg::Paint::RadialGradient(_)
            | usvg::Paint::Pattern(_) => self.tintable = false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::icons::icon_atlas::{IconAtlas, IconId};
    use glam::Vec2;

    const ONE_COLOUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><rect width="24" height="12" fill="#4080c0"/><circle cx="6" cy="6" r="3" fill="#4080c0"/></svg>"##;
    const TWO_COLOURS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#f00"/><rect x="8" width="8" height="16" fill="#00f"/></svg>"##;
    const GRADIENT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs><rect width="16" height="16" fill="url(#g)"/></svg>"##;
    const FILTERED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><filter id="f"><feGaussianBlur stdDeviation="1"/></filter></defs><g filter="url(#f)"><rect width="16" height="16" fill="#333"/></g></svg>"##;
    /// A stroke in a *second* colour: strokes have to be surveyed too, or an
    /// outlined two-tone icon would be mis-read as tintable.
    const STROKE_DIFFERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="#111" stroke="#eee" stroke-width="2"/></svg>"##;
    /// `fill="none"` on every path, with one stroke colour — the line-icon
    /// shape, and the case that must come out tintable or theming an outline
    /// set silently stops working.
    const OUTLINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" fill="none" stroke="#fff" stroke-width="1.8"/></svg>"##;
    /// The same artwork with the `fill="none"` left off one path. SVG's
    /// default fill is black, so that path carries a second paint colour even
    /// though its subpaths enclose no area and nothing black is ever drawn.
    /// A survey that only looked at strokes would call this tintable and then
    /// theming it would do nothing.
    const OUTLINE_DEFAULT_FILL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" stroke="#fff" stroke-width="1.8"/></svg>"##;

    /// One parse decides the three facts an `IconDef` carries that the markup
    /// does not state outright.
    #[test]
    fn runtime_build_derives_viewbox_tintability_and_filters() {
        let atlas = IconAtlas::from_svgs([
            ("two", TWO_COLOURS),
            ("one", ONE_COLOUR),
            ("stroke", STROKE_DIFFERS),
            ("gradient", GRADIENT),
            ("filtered", FILTERED),
            ("outline", OUTLINE),
            ("default-fill", OUTLINE_DEFAULT_FILL),
        ]);

        // Sorted by name, whatever order they arrived in — the invariant
        // `by_name`'s binary search rests on.
        let names: Vec<&str> = atlas.icons().iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            [
                "default-fill",
                "filtered",
                "gradient",
                "one",
                "outline",
                "stroke",
                "two"
            ]
        );

        let def = |name: &str| {
            atlas
                .icons()
                .iter()
                .find(|d| d.name == name)
                .unwrap_or_else(|| panic!("{name} missing"))
        };

        // viewBox comes off the parsed tree, not a caller-supplied guess.
        assert_eq!(def("one").view_box, Vec2::new(24.0, 12.0));
        assert_eq!(def("two").view_box, Vec2::new(16.0, 16.0));

        assert!(def("one").tintable, "one colour across two shapes");
        assert!(!def("two").tintable, "two fills, two colours");
        assert!(!def("gradient").tintable, "a gradient is not one colour");
        assert!(!def("stroke").tintable, "the stroke is a second colour");
        assert!(def("outline").tintable, "fill=none plus one stroke colour");
        assert!(
            !def("default-fill").tintable,
            "an omitted fill is black, not absent",
        );

        assert!(def("filtered").filtered);
        for name in [
            "one",
            "two",
            "gradient",
            "stroke",
            "outline",
            "default-fill",
        ] {
            assert!(!def(name).filtered, "{name} has no filter");
        }
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
