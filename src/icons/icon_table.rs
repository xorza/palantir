//! The baked icon-set data: an icon's definition, and the name-sorted table plus one SVG
//! blob.

use crate::common::span::Span;
use crate::icons::error::IconTableError;
use crate::icons::svg_facts::SvgFacts;
use crate::primitives::identity::icon_id::IconId;
use glam::Vec2;
use std::borrow::Cow;

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
    /// (the most an [`IconId`] can name), two share a name, or the sources total
    /// 4 GiB or more, past what a span's `u32` offsets address.
    pub fn from_svgs<'a, N: Into<Cow<'static, str>>>(
        sources: impl IntoIterator<Item = (N, &'a str)>,
    ) -> Result<Self, IconTableError> {
        Self::from_named_svgs(
            sources
                .into_iter()
                .map(|(name, svg)| (name.into(), svg))
                .collect(),
        )
    }

    fn from_named_svgs(sources: Vec<(Cow<'static, str>, &str)>) -> Result<Self, IconTableError> {
        let mut surveyed: Vec<(Cow<'static, str>, &str, SvgFacts)> =
            Vec::with_capacity(sources.len());
        for (name, svg) in sources {
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

        let blob_len = blob_len(surveyed.iter().map(|(_, svg, _)| svg.len()))
            .map_err(|bytes| IconTableError::TooLarge { bytes })?;
        let mut blob: Vec<u8> = Vec::with_capacity(blob_len);
        let mut icons: Vec<IconDefinition> = Vec::with_capacity(surveyed.len());
        for (name, svg, facts) in surveyed {
            let start = u32::try_from(blob.len()).expect("inside the blob checked above");
            let len = u32::try_from(svg.len()).expect("inside the blob checked above");
            blob.extend_from_slice(svg.as_bytes());
            icons.push(IconDefinition {
                name,
                view_box: facts.view_box,
                svg: Span::new(start, len),
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

    /// Every icon with the id that names it, in table order.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (IconId, &IconDefinition)> {
        self.icons.iter().enumerate().map(|(i, def)| {
            let index = u16::try_from(i).expect("an IconId names every icon of a set");
            (IconId::new(index), def)
        })
    }

    /// The icon named `name` (binary search; no allocation).
    pub(crate) fn by_name(&self, name: &str) -> Option<IconId> {
        self.icons
            .binary_search_by(|def| def.name.as_ref().cmp(name))
            .ok()
            .map(|i| IconId::new(u16::try_from(i).expect("an IconId names every icon of a set")))
    }

    /// The definition behind `icon`.
    ///
    /// # Panics
    ///
    /// Panics if `icon` is not from this set: an id crossed sets.
    pub(crate) fn def(&self, icon: IconId) -> &IconDefinition {
        let icons = &self.icons;
        let index = usize::from(icon.index());
        assert!(
            index < icons.len(),
            "IconId({}) is not in this set ({} icons) — an id from another set?",
            icon.index(),
            icons.len(),
        );
        &icons[index]
    }

    pub(crate) fn svg_bytes(&self, icon: IconId) -> &[u8] {
        &self.svg[self.def(icon).svg.range()]
    }
}

/// The length of the blob that concatenates sources of `lens` bytes, while every
/// span into it can end within `u32`; otherwise the exact total.
fn blob_len(lens: impl IntoIterator<Item = usize>) -> Result<usize, u128> {
    let total: u128 = lens
        .into_iter()
        .map(|len| u128::try_from(len).expect("a usize fits u128"))
        .sum();
    let len = u32::try_from(total).map_err(|_| total)?;
    Ok(usize::try_from(len).expect("a u32 fits the usize of a supported target"))
}

#[cfg(test)]
mod tests {
    use crate::icons::icon_table::{IconTable, blob_len};
    use crate::icons::internals::{BROKEN, ONE_COLOUR, TWO_COLOURS};
    use crate::primitives::identity::icon_id::IconId;
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

    /// Each icon's span must slice its own source out of the shared blob, and
    /// the ids the table mints name the sorted slots.
    #[test]
    fn spans_slice_each_icon_out_of_the_shared_blob() {
        let table = IconTable::from_svgs([("b", TWO_COLOURS), ("a", ONE_COLOUR)]).unwrap();
        assert_eq!(table.svg_bytes(IconId::new(0)), ONE_COLOUR.as_bytes());
        assert_eq!(table.svg_bytes(IconId::new(1)), TWO_COLOURS.as_bytes());
        assert_eq!(table.by_name("a"), Some(IconId::new(0)));
        assert_eq!(table.by_name("b"), Some(IconId::new(1)));
        assert_eq!(table.by_name("c"), None);
        let minted: Vec<(IconId, &str)> = table
            .entries()
            .map(|(id, def)| (id, def.name.as_ref()))
            .collect();
        assert_eq!(minted, [(IconId::new(0), "a"), (IconId::new(1), "b")]);
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

    /// The blob may end exactly at `u32::MAX`, the last end a span's `start + len`
    /// can hold; one byte more is an error that reports the exact total. Checked on
    /// lengths alone, as the sources themselves would take 4 GiB.
    #[test]
    fn blob_length_stops_at_the_last_end_a_span_can_hold() {
        use crate::icons::error::IconTableError;

        let max = usize::try_from(u32::MAX).unwrap();
        let half = 1 << 31;
        assert_eq!(blob_len([]), Ok(0));
        assert_eq!(blob_len([max]), Ok(max));
        // 2^31 + (2^31 - 1) = 2^32 - 1 = u32::MAX.
        assert_eq!(blob_len([half, half - 1]), Ok(max));
        // 2^31 + 2^31 = 2^32, one past.
        assert_eq!(blob_len([half, half]), Err(1 << 32));
        assert_eq!(blob_len([max, 1]), Err(1 << 32));
        // 3 * (2^32 - 1) = 12_884_901_885: past u32, reported whole.
        assert_eq!(blob_len([max; 3]), Err(12_884_901_885));

        assert_eq!(
            IconTableError::TooLarge { bytes: 1 << 32 }.to_string(),
            "an icon set holds at most 4294967295 bytes of SVG, got 4294967296"
        );
    }
}
