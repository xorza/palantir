//! The survey parse of an icon's SVG and the parse settings every parse shares; the only place outside the rasterizer that talks to `usvg`.

use glam::Vec2;
use resvg::usvg;

/// The two facts an [`IconDefinition`](crate::IconDefinition) carries that the markup does not state outright, read off one parse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SvgFacts {
    pub(crate) view_box: Vec2,
    /// Every paint resolved to at most one color, so a draw's tint can supply it.
    pub(crate) tintable: bool,
}

/// The parse settings every icon is read under; this survey and the rasterizer's tree must be configured identically.
pub(crate) fn parse_options() -> usvg::Options<'static> {
    usvg::Options::default()
}

impl SvgFacts {
    /// Parse `svg` and survey it.
    pub(crate) fn of(svg: &[u8]) -> Result<Self, usvg::Error> {
        let tree = usvg::Tree::from_data(svg, &parse_options())?;
        let mut survey = Survey {
            tintable: true,
            only: None,
        };
        survey.walk(tree.root());
        Ok(Self {
            view_box: Vec2::new(tree.size().width(), tree.size().height()),
            tintable: survey.tintable,
        })
    }
}

/// One walk's running state; `only` is scaffolding decided before `view_box` is known.
#[derive(Debug)]
struct Survey {
    tintable: bool,
    /// The one color seen so far.
    only: Option<usvg::Color>,
}

impl Survey {
    fn walk(&mut self, group: &usvg::Group) {
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
                // A raster image or unresolved text carries color that cannot be tinted away.
                usvg::Node::Image(_) | usvg::Node::Text(_) => self.tintable = false,
            }
        }
    }

    /// Fold one paint into the survey: a second color, a gradient or a pattern means the artwork's colors must survive.
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
    use crate::icons::internals::{BROKEN, ONE_COLOR, TWO_COLORS};
    use crate::icons::svg_facts::SvgFacts;
    use glam::Vec2;

    const GRADIENT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs><rect width="16" height="16" fill="url(#g)"/></svg>"##;
    /// A stroke in a second color: strokes are surveyed too.
    const STROKE_DIFFERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="#111" stroke="#eee" stroke-width="2"/></svg>"##;
    /// `fill="none"` on every path with one stroke color: the line-icon shape, which must stay tintable.
    const OUTLINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" fill="none" stroke="#fff" stroke-width="1.8"/></svg>"##;
    /// `fill="none"` left off one path: SVG's default fill is black, so that path carries a second color though nothing black is drawn; the survey must not call it tintable.
    const OUTLINE_DEFAULT_FILL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M12 11v7M8.5 14.5h7" stroke="#fff" stroke-width="1.8"/></svg>"##;

    fn facts(svg: &str) -> SvgFacts {
        SvgFacts::of(svg.as_bytes()).expect("fixture parses")
    }

    #[test]
    fn survey_reads_viewbox_and_tintability() {
        assert_eq!(facts(ONE_COLOR).view_box, Vec2::new(24.0, 12.0));
        assert_eq!(facts(TWO_COLORS).view_box, Vec2::new(16.0, 16.0));

        assert!(facts(ONE_COLOR).tintable, "one color across two shapes");
        assert!(!facts(TWO_COLORS).tintable, "two fills, two colors");
        assert!(!facts(GRADIENT).tintable, "a gradient is not one color");
        assert!(
            !facts(STROKE_DIFFERS).tintable,
            "the stroke is a second color",
        );
        assert!(facts(OUTLINE).tintable, "fill=none plus one stroke color");
        assert!(
            !facts(OUTLINE_DEFAULT_FILL).tintable,
            "an omitted fill is black, not absent",
        );
    }

    #[test]
    fn unparseable_source_has_no_facts() {
        assert!(SvgFacts::of(BROKEN.as_bytes()).is_err());
    }
}
