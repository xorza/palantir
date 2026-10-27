//! The numeric target a value widget writes through.

use crate::primitives::text::interned_str::InternedStr;
use crate::ui::Ui;

/// The numeric target a value widget writes through: an `i64` or `f64`, borrowed
/// mutably. Built through `From`, e.g. `DragValue::new(&mut my_i64)`.
#[derive(Debug)]
pub enum DragNum<'a> {
    /// An integer target.
    I64(&'a mut i64),
    /// A float target. A NaN or infinite value shows as it is, and a scrub or a key moves it
    /// from `0.0`, so the first write lands on a number inside the bounds.
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
            Self::I64(v) => v as f64,
            Self::F64(v) => v,
        }
    }
}

/// How a value widget stores a number: clamped into `lo..=hi`, a float target snapped
/// to `decimals` fractional digits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoreRule {
    lo: f64,
    hi: f64,
    decimals: usize,
}

impl StoreRule {
    /// The rule for a widget bounded by `min` and `max` in either order. Ordered with float
    /// `min`/`max`, which return the other operand for a NaN bound, so a degenerate pair still
    /// clamps to a number.
    pub(crate) const fn new(min: f64, max: f64, decimals: usize) -> Self {
        debug_assert!(
            decimals <= MAX_DECIMALS,
            "decimals is coerced at the setter"
        );
        Self {
            lo: min.min(max),
            hi: min.max(max),
            decimals,
        }
    }

    /// `value` pulled into the bounds, through `max`/`min` rather than `clamp`, which asserts
    /// ordered bounds and trips on NaN.
    const fn clamp(self, value: f64) -> f64 {
        value.max(self.lo).min(self.hi)
    }
}

/// The most fractional digits an `f64` holds; more would print float noise and overflow
/// `10^decimals`. A widget's `decimals` setter coerces to it.
pub(crate) const MAX_DECIMALS: usize = 15;

impl DragNum<'_> {
    /// The value a gesture moves from. A non-finite float reads as `0.0`, the neutral, so a
    /// scrub or a step from a NaN or infinite binding lands on a number inside the bounds; a NaN
    /// anchor in an unbounded range would clamp to `-inf`.
    pub(crate) const fn read(&self) -> Num {
        match self {
            DragNum::I64(v) => Num::I64(**v),
            DragNum::F64(v) if v.is_finite() => Num::F64(**v),
            DragNum::F64(_) => Num::F64(0.0),
        }
    }

    /// The stored value bit for bit, which [`Self::restore`] writes back.
    pub(crate) const fn stored(&self) -> Num {
        match self {
            DragNum::I64(v) => Num::I64(**v),
            DragNum::F64(v) => Num::F64(**v),
        }
    }

    /// Commit a scrubbed value: `anchor` moved by `offset`, stored under `rule`. The
    /// integer target adds the nearest whole step; the float target snaps to
    /// `rule.decimals` (`1.98457…` at 3 → `1.985`). Returns whether the stored value
    /// changed.
    pub(crate) fn commit_drag(&mut self, anchor: Num, offset: f64, rule: StoreRule) -> bool {
        match (self, anchor) {
            (DragNum::I64(v), Num::I64(a)) => {
                store_i64(v, a.saturating_add(whole_step(offset)), rule)
            }
            (DragNum::I64(v), Num::F64(a)) => store_i64(v, whole_step(a + offset), rule),
            (DragNum::F64(v), a) => store_f64(
                v,
                round_to_decimals(a.widen() + offset, rule.decimals),
                rule,
            ),
        }
    }

    /// Commit an absolute `value` named by a track position; the integer target
    /// rounds it whole.
    pub(crate) fn commit_value(&mut self, value: f64, rule: StoreRule) -> bool {
        self.commit_drag(Num::F64(value), 0.0, rule)
    }

    /// One unit of the last shown digit: `1` for the integer target, `10^-decimals`
    /// for the float one.
    pub(crate) fn unit(&self, decimals: usize) -> f64 {
        match self {
            DragNum::I64(_) => 1.0,
            DragNum::F64(_) => 1.0 / decimal_shift(decimals),
        }
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

    /// The value at `decimals` places, then `suffix`, interned for this frame.
    pub(crate) fn label(&self, ui: &mut Ui, decimals: usize, suffix: &str) -> InternedStr {
        match self {
            DragNum::I64(v) => ui.fmt(format_args!("{}{suffix}", **v)),
            DragNum::F64(v) => ui.fmt(format_args!("{:.*}{suffix}", decimals, **v)),
        }
    }

    pub(crate) fn edit_string(&self) -> String {
        match self {
            DragNum::I64(v) => v.to_string(),
            DragNum::F64(v) => format!("{:?}", **v),
        }
    }

    /// Parse `text` and write it clamped into `rule`'s bounds, keeping the typed digits past
    /// `rule`'s decimals. Unparseable or non-finite text leaves the value alone, since a
    /// committed NaN would poison every later scrub.
    pub(crate) fn parse_from(&mut self, text: &str, rule: StoreRule) -> bool {
        match self {
            DragNum::I64(v) => match text.parse::<i64>() {
                // Parsed as `i64`: widening through `f64` would lose low bits past
                // 2^53.
                Ok(n) => store_i64(v, n, rule),
                Err(_) => false,
            },
            DragNum::F64(v) => match text.parse::<f64>() {
                Ok(n) if n.is_finite() => store_f64(v, n, rule),
                _ => false,
            },
        }
    }
}

/// Store `next` clamped into `rule`'s bounds, answering whether the value moved. The
/// bounds close in to the integers inside them (`0.5..=10.0` stores no 0, where a
/// truncating cast let it through); a range holding no integer clamps to the two
/// either side.
fn store_i64(slot: &mut i64, next: i64, rule: StoreRule) -> bool {
    let (lo, hi) = (rule.lo.ceil() as i64, rule.hi.floor() as i64);
    let next = next.clamp(lo.min(hi), hi.max(lo));
    let changed = *slot != next;
    *slot = next;
    changed
}

/// The float half of that write. `+ 0.0` turns `-0.0` into `+0.0`, which the clamp's `<`
/// would leak as "-0.00"; the comparison is bit-exact.
fn store_f64(slot: &mut f64, next: f64, rule: StoreRule) -> bool {
    let next = rule.clamp(next) + 0.0;
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
    let p = decimal_shift(decimals);
    let shifted = v * p;
    if shifted.abs() < WHOLE_ONLY {
        shifted.round() / p
    } else {
        v
    }
}

/// `10^decimals`; `decimals` is at most [`MAX_DECIMALS`].
fn decimal_shift(decimals: usize) -> f64 {
    debug_assert!(
        decimals <= MAX_DECIMALS,
        "decimals is coerced at the setter"
    );
    10f64.powi(decimals as i32)
}

#[cfg(test)]
mod tests;
