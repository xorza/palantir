//! SVG documents the icon tests share.

/// One fill colour over a 24 × 12 viewBox — a tintable icon.
pub(crate) const ONE_COLOUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><rect width="24" height="12" fill="#4080c0"/><circle cx="6" cy="6" r="3" fill="#4080c0"/></svg>"##;

/// Two fill colours over a 16 × 16 viewBox — a multi-colour icon.
pub(crate) const TWO_COLOURS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#f00"/><rect x="8" width="8" height="16" fill="#00f"/></svg>"##;

/// A document cut off after its opening — what a parse rejects.
pub(crate) const BROKEN: &str = "<svg";
