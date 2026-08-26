pub(crate) const MAX_PACKED_GAP: f32 = 65_504.0;

#[inline]
pub(crate) const fn valid_lower_bound(value: f32) -> bool {
    value >= 0.0 && value < f32::INFINITY
}

#[inline]
pub(crate) const fn valid_upper_bound(value: f32) -> bool {
    value >= 0.0
}

#[inline]
pub(crate) const fn valid_gap(value: f32) -> bool {
    valid_lower_bound(value)
}

#[inline]
pub(crate) const fn valid_packed_gap(value: f32) -> bool {
    valid_gap(value) && value <= MAX_PACKED_GAP
}

#[cfg(test)]
mod tests;
