//! The numeric target a value widget writes through.

pub(crate) mod limits;

use crate::widgets::drag_num::limits::Limits;

/// The numeric target a value widget writes through: an `i64` or `f64`, borrowed
/// mutably. Built through `From`, e.g. `DragValue::new(&mut my_i64)`.
#[derive(Debug)]
pub enum DragNum<'a> {
    /// An integer target.
    I64(&'a mut i64),
    /// A float target.
    F64(&'a mut f64),
}

/// A value read out of a [`DragNum`] at its stored precision: the owned anchor a
/// gesture holds across frames. An `i64` stays an `i64` because widening past 2^53
/// would store a rounded number over the exact one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Num {
    I64(i64),
    F64(f64),
}

impl Num {
    /// This value as an `f64` for visual quantities; lossy past 2^53, so no
    /// write-back path uses it.
    pub(crate) const fn widen(self) -> f64 {
        match self {
            Num::I64(v) => v as f64,
            Num::F64(v) => v,
        }
    }
}

impl DragNum<'_> {
    pub(crate) const fn read(&self) -> Num {
        match self {
            DragNum::I64(v) => Num::I64(**v),
            DragNum::F64(v) => Num::F64(**v),
        }
    }

    /// Commit a scrubbed value: `anchor` moved by `offset`, clamped into `[min,
    /// max]`. The integer target adds the nearest whole step; the float target
    /// snaps to `decimals` (`1.98457…` at 3 → `1.985`). Returns whether the stored
    /// value changed.
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

    /// Commit an absolute `value` named by a track position; the integer target
    /// rounds it whole.
    pub(crate) fn commit_value(&mut self, value: f64, decimals: usize, min: f64, max: f64) -> bool {
        self.commit_drag(Num::F64(value), 0.0, decimals, min, max)
    }

    /// Write back `value` exactly, undoing a canceled edit. Returns whether it
    /// changed.
    pub(crate) fn restore(&mut self, value: Num) -> bool {
        match (self, value) {
            (DragNum::I64(v), Num::I64(n)) => {
                let changed = **v != n;
                **v = n;
                changed
            }
            (DragNum::F64(v), Num::F64(n)) => {
                let changed = v.to_bits() != n.to_bits();
                **v = n;
                changed
            }
            (_, value) => unreachable!("{value:?} restored into a binding of the other type"),
        }
    }

    pub(crate) fn edit_string(&self) -> String {
        match self {
            DragNum::I64(v) => v.to_string(),
            DragNum::F64(v) => format!("{:?}", **v),
        }
    }

    /// Parse `text` and write it clamped into `[min, max]`. Unparseable or
    /// non-finite text leaves the value alone, since a committed NaN would poison
    /// every later scrub.
    pub(crate) fn parse_from(&mut self, text: &str, min: f64, max: f64) -> bool {
        let limits = Limits::of(min, max);
        match self {
            DragNum::I64(v) => match text.parse::<i64>() {
                // Parsed as `i64`: widening through `f64` would lose low bits past
                // 2^53.
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

/// Store `next` clamped into `limits`, answering whether the value moved. The
/// bounds close in to the integers inside them (`0.5..=10.0` stores no 0, where a
/// truncating cast let it through); a range holding no integer clamps to the two
/// either side.
fn store_i64(slot: &mut i64, next: i64, limits: Limits<f64>) -> bool {
    let (lo, hi) = (limits.lo.ceil() as i64, limits.hi.floor() as i64);
    let next = next.clamp(lo.min(hi), hi.max(lo));
    let changed = *slot != next;
    *slot = next;
    changed
}

/// The float half of that write. Uses [`Limits::clamp`], which tolerates the
/// all-NaN pair the inherent clamp asserts on. `+ 0.0` turns `-0.0` into `+0.0`,
/// which the clamp's `<` would leak as "-0.00"; the comparison is bit-exact.
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

/// The whole step nearest `offset`; the saturating cast pins huge travel and maps
/// NaN to 0.
const fn whole_step(offset: f64) -> i64 {
    offset.round() as i64
}

const WHOLE_ONLY: f64 = (1u64 << f64::MANTISSA_DIGITS) as f64;

/// Round `v` to `decimals` fractional digits. Dividing by `10^decimals` lands on
/// the nearest f64 to a short decimal. Past [`WHOLE_ONLY`] the value is returned as
/// is, as shifting `1e308` would overflow to infinity.
fn round_to_decimals(v: f64, decimals: usize) -> f64 {
    // Capped at 15 digits: f64 holds no more, and 10^decimals would overflow.
    let p = 10f64.powi(decimals.min(15) as i32);
    let shifted = v * p;
    if shifted.abs() < WHOLE_ONLY {
        shifted.round() / p
    } else {
        v
    }
}

#[cfg(test)]
mod tests;
