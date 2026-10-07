//! The survey parse of an icon's SVG and the parse settings every parse shares; the only place outside the rasterizer that talks to `usvg`.

use glam::Vec2;
use resvg::usvg;

/// The three facts an [`IconDefinition`](crate::IconDefinition) carries that the markup does not state outright, read off one parse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SvgFacts {
    pub(crate) view_box: Vec2,
    /// Every paint resolved to at most one colour, so a draw's tint can supply it.
    pub(crate) tintable: bool,
    /// Some group carries an SVG filter: 10-20x raster cost.
    pub(crate) filtered: bool,
}

/// The parse settings every icon is read under; this survey and the rasterizer's tree must be configured identically.
pub(crate) fn parse_options() -> usvg::Options<'static> {
    usvg::Options::default()
}

impl SvgFacts {
    /// Parse `svg` and survey it; `None` when it does not parse.
    pub(crate) fn of(svg: &[u8]) -> Option<Self> {
        let tree = usvg::Tree::from_data(svg, &parse_options()).ok()?;
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

/// One walk's running state; `only` is scaffolding decided before `view_box` is known.
#[derive(Debug)]
struct Survey {
    tintable: bool,
    filtered: bool,
    /// The one colour seen so far.
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
                // A raster image or unresolved text carries colour that cannot be tinted away.
                usvg::Node::Image(_) | usvg::Node::Text(_) => self.tintable = false,
            }
        }
    }

    /// Fold one paint into the survey: a second colour, a gradient or a pattern means the artwork's colours must survive.
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
    use crate::icons::internals::{BROKEN, ONE_COLOUR, TWO_COLOURS};
    use crate::icons::svg_facts::SvgFacts;
    use glam::Vec2;

    const GRADIENT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs><rect width="16" height="16" fill="url(#g)"/></svg>"##;
    const FILTERED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><filter id="f"><feGaussianBlur stdDeviation="1"/></filter></defs><g filter="url(#f)"><rect width="16" height="16" fill="#333"/></g></svg>"##;
    /// A stroke in a second colour: strokes are surveyed too.
    const STROKE_DIFFERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="#111" stroke="#eee" stroke-width="2"/></svg>"##;
    /// `fill="none"` on every path with one stroke colour: the line-icon shape, which must stay tintable.
    const OUTLINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" fill="none" stroke="#fff" stroke-width="1.8"/></svg>"##;
    /// `fill="none"` left off one path: SVG's default fill is black, so that path carries a second colour though nothing black is drawn; the survey must not call it tintable.
    const OUTLINE_DEFAULT_FILL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" stroke="#fff" stroke-width="1.8"/></svg>"##;

    fn facts(svg: &str) -> SvgFacts {
        SvgFacts::of(svg.as_bytes()).expect("fixture parses")
    }

    #[test]
    fn survey_reads_viewbox_tintability_and_filter_use() {
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
        assert_eq!(SvgFacts::of(BROKEN.as_bytes()), None);
    }
}
