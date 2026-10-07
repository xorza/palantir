//! The baked icon-set data: an icon's id, its definition, and the name-sorted table
//! plus one SVG blob.

use crate::common::span::Span;
use crate::icons::error::IconTableError;
use crate::icons::svg_facts::SvgFacts;
use glam::Vec2;
use std::borrow::Cow;

/// Index of one icon within its [`IconTable`]. A generated set emits a named
/// constant per icon (`icons::SAVE`); a runtime-built one resolves through
/// [`IconSet::by_name`](crate::IconSet::by_name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconId(pub u16);

/// One baked icon: its name, the size it was drawn at, where its normalized SVG
/// sits in the set's blob, and two facts the renderer needs before parsing.
#[derive(Clone, Debug)]
pub struct IconDefinition {
    /// Lowercase kebab-case, from the source filename; unique within a set and the
    /// key [`IconSet::by_name`](crate::IconSet::by_name) searches.
    pub name: Cow<'static, str>,
    /// The SVG's viewBox extent in logical px.
    pub view_box: Vec2,
    /// Byte range of this icon's SVG within its set's blob.
    pub svg: Span,
    /// Every paint resolves to one colour (or `currentColor`), so it rasterizes to
    /// a coverage mask and takes the shape's full tint; a colour icon rasterizes to
    /// RGBA, where tint modulates alpha only.
    pub tintable: bool,
    /// The icon uses an SVG filter (10-20x costlier to rasterize), so it is
    /// prewarmed at load rather than on its first draw.
    pub filtered: bool,
}

/// An icon set: a name-sorted table (for
/// [`IconSet::by_name`](crate::IconSet::by_name)'s binary search) plus every icon's
/// SVG in one blob. [`Self::baked`] borrows data compiled into the binary and
/// parses nothing; [`Self::from_svgs`] builds one at runtime, owns its buffers and
/// reads each source to fill the table. Rasterization parses lazily per icon on
/// first draw either way.
#[derive(Debug)]
pub struct IconTable {
    icons: Cow<'static, [IconDefinition]>,
    svg: Cow<'static, [u8]>,
}

impl IconTable {
    /// A set from data compiled into the binary, as a generated `icons.rs` calls
    /// it; borrows both halves and parses nothing. `icons` must be sorted by
    /// [`IconDefinition::name`] and each entry's [`IconDefinition::svg`] must span
    /// its own slice of `svg`; the generator owes this, [`Self::from_svgs`]
    /// establishes it.
    pub const fn baked(icons: &'static [IconDefinition], svg: &'static [u8]) -> Self {
        Self {
            icons: Cow::Borrowed(icons),
            svg: Cow::Borrowed(svg),
        }
    }

    /// Build a set from SVG sources at runtime, deriving each icon's viewBox,
    /// tintability and filter use from one parse. It pays a parse per icon, so a
    /// shipped set should use [`Self::baked`]. **That parse is paid twice for any
    /// icon later drawn** (`IconRasterizer` parses again); retaining the tree would
    /// put a parser type in a data type, which costs more than ~180 µs once.
    /// Entries are sorted by name, so resolve ids by name, not input order.
    ///
    /// # Errors
    ///
    /// [`IconTableError`] when a source does not parse, there are more than 65 536
    /// (the most an [`IconId`] can name), or two share a name.
    pub fn from_svgs<'a, N: Into<Cow<'static, str>>>(
        sources: impl IntoIterator<Item = (N, &'a str)>,
    ) -> Result<Self, IconTableError> {
        let mut surveyed: Vec<(Cow<'static, str>, &'a str, SvgFacts)> = Vec::new();
        for (name, svg) in sources {
            let name = name.into();
            let Some(facts) = SvgFacts::of(svg.as_bytes()) else {
                return Err(IconTableError::Unreadable { name });
            };
            surveyed.push((name, svg, facts));
        }
        if surveyed.len() > usize::from(u16::MAX) + 1 {
            return Err(IconTableError::TooMany {
                count: surveyed.len(),
            });
        }
        surveyed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if let Some(pair) = surveyed.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(IconTableError::DuplicateName {
                name: pair[0].0.clone(),
            });
        }

        let mut blob: Vec<u8> = Vec::with_capacity(surveyed.iter().map(|(_, s, _)| s.len()).sum());
        let mut icons: Vec<IconDefinition> = Vec::with_capacity(surveyed.len());
        for (name, svg, facts) in surveyed {
            let start = blob.len() as u32;
            blob.extend_from_slice(svg.as_bytes());
            icons.push(IconDefinition {
                name,
                view_box: facts.view_box,
                svg: Span::new(start, svg.len() as u32),
                tintable: facts.tintable,
                filtered: facts.filtered,
            });
        }
        Ok(Self {
            icons: Cow::Owned(icons),
            svg: Cow::Owned(blob),
        })
    }

    pub(crate) fn icons(&self) -> &[IconDefinition] {
        &self.icons
    }

    /// The definition behind `icon`.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set: an id crossed sets.
    pub(crate) fn def(&self, icon: IconId) -> &IconDefinition {
        let icons = &self.icons;
        assert!(
            (icon.0 as usize) < icons.len(),
            "IconId({}) is not in this set ({} icons) — an id from another set?",
            icon.0,
            icons.len(),
        );
        &icons[icon.0 as usize]
    }

    pub(crate) fn svg_bytes(&self, icon: IconId) -> &[u8] {
        &self.svg[self.def(icon).svg.range()]
    }
}

#[cfg(test)]
mod tests {
    use crate::icons::icon_table::{IconId, IconTable};
    use crate::icons::internals::{BROKEN, ONE_COLOUR, TWO_COLOURS};
    use glam::Vec2;

    /// `from_svgs` sorts by name, which `by_name`'s binary search rests on.
    #[test]
    fn runtime_build_sorts_by_name() {
        let table = IconTable::from_svgs([("two", TWO_COLOURS), ("one", ONE_COLOUR)]).unwrap();
        let names: Vec<&str> = table.icons().iter().map(|d| d.name.as_ref()).collect();
        assert_eq!(
            names,
            ["one", "two"],
            "sorted, whatever order they arrived in"
        );
        assert_eq!(table.icons()[0].view_box, Vec2::new(24.0, 12.0));
        assert!(table.icons()[0].tintable, "facts ride through to the def");
    }

    /// Each icon's span must slice its own source out of the shared blob.
    #[test]
    fn spans_slice_each_icon_out_of_the_shared_blob() {
        let table = IconTable::from_svgs([("b", TWO_COLOURS), ("a", ONE_COLOUR)]).unwrap();
        assert_eq!(table.svg_bytes(IconId(0)), ONE_COLOUR.as_bytes());
        assert_eq!(table.svg_bytes(IconId(1)), TWO_COLOURS.as_bytes());
    }

    const TINY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;

    /// Each flaw is its own error: an unparseable source names its icon, too many
    /// sources report the count, a shared name names itself; an owned name is
    /// accepted beside a borrowed one.
    #[test]
    fn flawed_sources_are_errors() {
        use crate::icons::error::IconTableError;

        assert_eq!(
            IconTable::from_svgs([("good", ONE_COLOUR), ("bad", BROKEN)]).err(),
            Some(IconTableError::Unreadable { name: "bad".into() }),
        );
        assert_eq!(
            IconTable::from_svgs([
                ("twin", ONE_COLOUR),
                ("other", ONE_COLOUR),
                ("twin", ONE_COLOUR),
            ])
            .err(),
            Some(IconTableError::DuplicateName {
                name: "twin".into()
            }),
        );
        let count = usize::from(u16::MAX) + 2;
        assert_eq!(
            IconTable::from_svgs((0..count).map(|i| (format!("i{i}"), TINY))).err(),
            Some(IconTableError::TooMany { count }),
        );
        let owned = IconTable::from_svgs([(String::from("owned"), TINY)]).unwrap();
        assert_eq!(owned.icons()[0].name, "owned");
    }
}
