//! What a measure's answer holds under: the offers the measure cache can serve it at.

/// `stable_from` of an axis that read its offer (Fill share, line break, width-bound text); no
/// finite offer reaches it, so the cache serves only the same offer.
pub(crate) const AT_OFFER_ONLY: f32 = f32::INFINITY;
