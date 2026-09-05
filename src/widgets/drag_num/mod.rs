//! The numeric target a value widget writes through.

pub(crate) mod limits;

use crate::widgets::drag_num::limits::Limits;

/// The numeric target a value widget writes through: either an `i64` or
/// an `f64`, borrowed mutably for the widget's lifetime. Build one
/// implicitly through `From` — `DragValue::new(&mut my_i64)` and
/// `Slider::new(&mut my_f64, 0.0..=1.0)` both work.
///
/// One binding for both widgets, so a caller who can scrub a number can
/// slide the same one. The math runs in `f64`; the integer case rounds
/// back to the nearest whole step on write.
#[derive(Debug)]
pub enum DragNum<'a> {
    I64(&'a mut i64),
    F64(&'a mut f64),
}

impl DragNum<'_> {
    /// The bound value widened to `f64` — captured as the drag anchor.
    pub(crate) fn get(&self) -> f64 {
        match self {
            DragNum::I64(v) => **v as f64,
            DragNum::F64(v) => **v,
        }
    }

    /// Commit a scrubbed `raw` value: the float target snaps to `decimals`
    /// (so a drag never stores a long tail — `1.98457…` at 3 → `1.985`), the
    /// integer target rounds to the nearest whole step; both clamp into
    /// `[min, max]` (a reversed pair is tolerated). Infinite bounds cast to
    /// `i64::MIN`/`MAX`, so an unbounded integer clamp is a no-op. Returns
    /// whether the stored value actually changed — exact for the integer,
    /// bit-exact for the float.
    pub(crate) fn commit_drag(&mut self, raw: f64, decimals: usize, min: f64, max: f64) -> bool {
        let limits = Limits::of(min, max);
        match self {
            DragNum::I64(v) => store_i64(v, raw.round() as i64, limits),
            DragNum::F64(v) => store_f64(v, round_to_decimals(raw, decimals), limits),
        }
    }

    /// Exact, full-precision text for the edit buffer — `{:?}` on the float
    /// keeps a trailing `.0` so a whole value still reads as a float.
    pub(crate) fn edit_string(&self) -> String {
        match self {
            DragNum::I64(v) => v.to_string(),
            DragNum::F64(v) => format!("{:?}", **v),
        }
    }

    /// Parse `text` and write it clamped into `[min, max]`, leaving the
    /// value untouched when the text doesn't parse (partial input like
    /// `"3."`) or parses non-finite — a committed NaN survives clamp and
    /// poisons every subsequent scrub, so `"nan"`/`"inf"` are rejected.
    /// Returns whether the stored value changed. Keyboard entry keeps full
    /// precision — only drags snap to `decimals`.
    pub(crate) fn parse_from(&mut self, text: &str, min: f64, max: f64) -> bool {
        let limits = Limits::of(min, max);
        match self {
            DragNum::I64(v) => match text.parse::<i64>() {
                // Parsed as an `i64` and stored as one: widening through
                // `f64` on the way would lose the low bits of anything
                // past 2^53, which a drag never reaches but typed text
                // can.
                Ok(n) => store_i64(v, n, limits),
                Err(_) => false,
            },
            DragNum::F64(v) => match text.parse::<f64>() {
                Ok(n) if n.is_finite() => store_f64(v, n, limits),
                _ => false,
            },
        }
    }
}

/// Store `next` clamped into `limits`, answering whether the stored
/// value moved.
///
/// The integer half of the one write every path through [`DragNum`] ends
/// in — a scrub commit and a typed edit alike. The bounds arrive as `f64`
/// and cast: an infinite bound becomes `i64::MIN`/`MAX`, so an unbounded
/// clamp is a no-op.
fn store_i64(slot: &mut i64, next: i64, limits: Limits<f64>) -> bool {
    let next = next.clamp(limits.lo as i64, limits.hi as i64);
    let changed = *slot != next;
    *slot = next;
    changed
}

/// The float half of that write.
///
/// Through [`Limits::clamp`] rather than the inherent one, which asserts
/// its bounds are ordered and so panics on the all-NaN pair `Limits::of`
/// cannot repair.
///
/// `+ 0.0` normalizes `-0.0` to `+0.0` (IEEE: `-0.0 + 0.0 = +0.0`):
/// rounding a small negative value yields `-0.0`, and the clamp's `<`
/// lets it slip through a `+0.0` lower bound — the sign would leak into
/// the display ("-0.00") and into serialized values. The comparison is
/// bit-exact, so what the caller is told changed is what the slot holds.
fn store_f64(slot: &mut f64, next: f64, limits: Limits<f64>) -> bool {
    let next = limits.clamp(next) + 0.0;
    let changed = slot.to_bits() != next.to_bits();
    *slot = next;
    changed
}

impl<'a> From<&'a mut i64> for DragNum<'a> {
    fn from(v: &'a mut i64) -> Self {
        DragNum::I64(v)
    }
}

impl<'a> From<&'a mut f64> for DragNum<'a> {
    fn from(v: &'a mut f64) -> Self {
        DragNum::F64(v)
    }
}

/// Round `v` to `decimals` fractional digits. Shifts by `10^decimals`,
/// rounds, and divides back — the divide-by-a-power-of-ten (rather than a
/// multiply by `10^-decimals`) lands on the nearest f64 to a short decimal,
/// so the result formats without a long tail (1.98457… at 3 → 1.985).
fn round_to_decimals(v: f64, decimals: usize) -> f64 {
    // `10^decimals` overflows to `inf` past ~308 digits (and `f64` carries no
    // more than ~15 anyway); clamp so the shift stays finite and the fn total.
    let p = 10f64.powi(decimals.min(15) as i32);
    (v * p).round() / p
}

#[cfg(test)]
mod tests;
