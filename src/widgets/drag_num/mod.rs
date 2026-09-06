//! The numeric target a value widget writes through.

pub(crate) mod limits;

use crate::widgets::drag_num::limits::Limits;

/// The numeric target a value widget writes through: either an `i64` or
/// an `f64`, borrowed mutably for the widget's lifetime. Build one
/// implicitly through `From` — `DragValue::new(&mut my_i64)` and
/// `Slider::new(&mut my_f64, 0.0..=1.0)` both work.
///
/// One binding for both widgets, so a caller who can scrub a number can
/// slide the same one. Each target keeps its own domain: an integer moves
/// by whole steps from the integer it holds, and a float keeps every
/// digit the caller stored.
#[derive(Debug)]
pub enum DragNum<'a> {
    I64(&'a mut i64),
    F64(&'a mut f64),
}

/// A value read out of a [`DragNum`] at the precision it is stored at:
/// the owned half of that borrowed pair.
///
/// The two shapes coincide and the types do not. [`DragNum`] borrows the
/// caller's number for one frame, while a gesture anchors on the value
/// it began with and holds that anchor for every frame until it ends —
/// so the anchor is owned, and this is what it is.
///
/// It is an `i64` and not an `f64` for the integer target, because the
/// gesture writes back through it. Widened, an `i64` past 2^53 comes
/// back rounded, and the gesture stores that rounded number over the
/// exact one typed entry accepted: a drag that moves a value nowhere
/// changes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Num {
    I64(i64),
    F64(f64),
}

impl Num {
    /// This value as an `f64`, for the visual quantities a fraction of a
    /// track is made of. Lossy past 2^53, which is why no path that
    /// writes the value back goes through it.
    pub(crate) fn widen(self) -> f64 {
        match self {
            Num::I64(v) => v as f64,
            Num::F64(v) => v,
        }
    }
}

impl DragNum<'_> {
    /// The bound value at the precision it is stored at — captured as the
    /// anchor a scrub moves from.
    pub(crate) fn read(&self) -> Num {
        match self {
            DragNum::I64(v) => Num::I64(**v),
            DragNum::F64(v) => Num::F64(**v),
        }
    }

    /// Commit a scrubbed value: `anchor` moved by `offset`, where
    /// `anchor` is the value the gesture began on and `offset` is its
    /// accumulated travel in value units.
    ///
    /// The integer target adds the nearest whole step to the integer it
    /// anchored on, so the anchor stays exact and a travel worth less
    /// than half a step stores nothing new. The float target snaps the
    /// sum to `decimals`, so a drag never stores a long tail —
    /// `1.98457…` at 3 → `1.985`. Both clamp into `[min, max]` (a
    /// reversed pair is tolerated). Infinite bounds cast to
    /// `i64::MIN`/`MAX`, so an unbounded integer clamp is a no-op.
    /// Returns whether the stored value actually changed — exact for the
    /// integer, bit-exact for the float.
    pub(crate) fn commit_drag(
        &mut self,
        anchor: Num,
        offset: f64,
        decimals: usize,
        min: f64,
        max: f64,
    ) -> bool {
        let limits = Limits::of(min, max);
        match (self, anchor) {
            (DragNum::I64(v), Num::I64(a)) => {
                store_i64(v, a.saturating_add(whole_step(offset)), limits)
            }
            (DragNum::I64(v), Num::F64(a)) => store_i64(v, whole_step(a + offset), limits),
            (DragNum::F64(v), a) => {
                store_f64(v, round_to_decimals(a.widen() + offset, decimals), limits)
            }
        }
    }

    /// Commit an absolute `value` — the number a track position names
    /// outright, rather than a move from where the gesture began. The
    /// pointer is a float quantity whatever the binding holds, so a
    /// position resolves through the float domain and the integer target
    /// rounds it whole.
    pub(crate) fn commit_value(&mut self, value: f64, decimals: usize, min: f64, max: f64) -> bool {
        self.commit_drag(Num::F64(value), 0.0, decimals, min, max)
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
                // past 2^53, which a drag anchored on the integer never
                // loses either.
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

/// The whole step nearest `offset`.
///
/// Rust's float→int cast saturates by language guarantee, so an enormous
/// or infinite travel pins to the end of the integer domain rather than
/// wrapping, and a NaN one moves the anchor nowhere.
fn whole_step(offset: f64) -> i64 {
    offset.round() as i64
}

/// The magnitude past which an `f64` holds whole numbers only, so no
/// decimal rounding is left to do: at 2^53 the spacing between
/// neighbours reaches 2, and it only grows.
const WHOLE_ONLY: f64 = (1u64 << f64::MANTISSA_DIGITS) as f64;

/// Round `v` to `decimals` fractional digits. Shifts by `10^decimals`,
/// rounds, and divides back — the divide-by-a-power-of-ten (rather than a
/// multiply by `10^-decimals`) lands on the nearest f64 to a short decimal,
/// so the result formats without a long tail (1.98457… at 3 → 1.985).
///
/// A value the shift cannot hold is returned as it stands. Past
/// [`WHOLE_ONLY`] the shifted value carries no fractional digits to
/// round, so the shift can only lose the value — `1e308` at 2 decimals
/// shifts to infinity, and the divide back never returns from it. That
/// is a finite number a drag turns infinite, and the bound below is what
/// keeps this total for every finite input.
fn round_to_decimals(v: f64, decimals: usize) -> f64 {
    // `10^decimals` overflows to `inf` past ~308 digits (and `f64` carries no
    // more than ~15 anyway); clamp so the shift stays finite and the fn total.
    let p = 10f64.powi(decimals.min(15) as i32);
    let shifted = v * p;
    // NaN fails the comparison and passes through, as it did through the
    // shift.
    match shifted.abs() < WHOLE_ONLY {
        true => shifted.round() / p,
        false => v,
    }
}

#[cfg(test)]
mod tests;
