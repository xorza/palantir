use glam::Vec2;
use resvg::usvg;

/// The three things an [`IconDef`](crate::IconDef) carries that the markup
/// does not state outright, read off one parse of an SVG source.
///
/// This is the only place outside the rasterizer that talks to `usvg`, which
/// is the point: [`IconAtlas`](crate::IconAtlas) is a data type and should not
/// have to know an SVG parser exists to build one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SvgFacts {
    /// The viewBox extent — the size the artwork was drawn at.
    pub(crate) view_box: Vec2,
    /// Every paint resolved to at most one colour, so the artwork carries no
    /// colour of its own worth keeping and a draw's tint can supply it.
    pub(crate) tintable: bool,
    /// Some group carries an SVG filter — 10-20x the raster cost, which is
    /// what the renderer prewarms against.
    pub(crate) filtered: bool,
}

impl SvgFacts {
    /// Parse `svg` and survey it. `None` when it does not parse — the caller
    /// drops the icon, since it could not have been rasterized either.
    pub(crate) fn of(svg: &[u8]) -> Option<Self> {
        let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
        let mut survey = Survey {
            tintable: true,
            filtered: false,
            only: None,
        };
        survey.walk(tree.root());
        Some(Self {
            view_box: Vec2::new(tree.size().width(), tree.size().height()),
            tintable: survey.tintable,
            filtered: survey.filtered,
        })
    }
}

/// One walk's running state. Separate from [`SvgFacts`] because `only` is
/// scaffolding — it decides `tintable` and is then thrown away — and because
/// the walk runs before `view_box` is known.
#[derive(Debug)]
struct Survey {
    tintable: bool,
    filtered: bool,
    /// The one colour seen so far, if the walk has seen exactly one. A second
    /// distinct colour clears `tintable` and this stops mattering.
    only: Option<usvg::Color>,
}

impl Survey {
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
    use crate::icons::svg_facts::SvgFacts;
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

    fn facts(svg: &str) -> SvgFacts {
        SvgFacts::of(svg.as_bytes()).expect("fixture parses")
    }

    /// One parse decides the three facts an `IconDef` carries that the markup
    /// does not state outright.
    #[test]
    fn survey_reads_viewbox_tintability_and_filter_use() {
        // viewBox comes off the parsed tree, not a caller-supplied guess.
        assert_eq!(facts(ONE_COLOUR).view_box, Vec2::new(24.0, 12.0));
        assert_eq!(facts(TWO_COLOURS).view_box, Vec2::new(16.0, 16.0));

        assert!(facts(ONE_COLOUR).tintable, "one colour across two shapes");
        assert!(!facts(TWO_COLOURS).tintable, "two fills, two colours");
        assert!(!facts(GRADIENT).tintable, "a gradient is not one colour");
        assert!(
            !facts(STROKE_DIFFERS).tintable,
            "the stroke is a second colour",
        );
        assert!(facts(OUTLINE).tintable, "fill=none plus one stroke colour");
        assert!(
            !facts(OUTLINE_DEFAULT_FILL).tintable,
            "an omitted fill is black, not absent",
        );

        assert!(facts(FILTERED).filtered);
        for svg in [
            ONE_COLOUR,
            TWO_COLOURS,
            GRADIENT,
            STROKE_DIFFERS,
            OUTLINE,
            OUTLINE_DEFAULT_FILL,
        ] {
            assert!(!facts(svg).filtered, "no filter in this one");
        }
    }

    #[test]
    fn unparseable_source_has_no_facts() {
        assert_eq!(SvgFacts::of(b"<svg"), None);
    }
}
