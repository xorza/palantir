//! The numeric target a value widget writes through.

use crate::primitives::math::libm_ext::Libm;
use crate::primitives::text::interned_str::InternedStr;
use crate::ui::Ui;
use std::ops::RangeInclusive;

/// The numeric target a value widget writes through: an `f32`, `f64`, `i32` or `i64`, borrowed
/// mutably, so a caller binds its own field with no cast. Built through `From`, e.g.
/// `DragValue::new(&mut my_i64)`. A closed set of the crate's scalars, as egui's `Numeric`
/// trait covers its own.
#[derive(Debug)]
pub enum BoundNum<'a> {
    /// A 32-bit integer target.
    I32(&'a mut i32),
    /// A 64-bit integer target.
    I64(&'a mut i64),
    /// A 32-bit float target, the crate's own scalar: a value snaps to its decimals in `f64`,
    /// then rounds to the nearest `f32`. A NaN or infinite value shows as it is, as an `F64`'s.
    F32(&'a mut f32),
    /// A 64-bit float target. A NaN or infinite value shows as it is, and a scrub or a key moves
    /// it from `0.0`, so the first write lands on a number inside the bounds.
    F64(&'a mut f64),
}

/// A value read out of a [`BoundNum`] at its stored precision: the owned anchor a
/// gesture holds across frames. An integer stays an `i64` because widening past 2^53
/// would store a rounded number over the exact one; an `f32` widens to `f64` exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Num {
    I64(i64),
    F64(f64),
}

impl Num {
    /// Whether `other` is this value bit for bit, so a float's `-0.0` and `0.0` differ.
    const fn same_bits(self, other: Self) -> bool {
        match (self, other) {
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::F64(a), Self::F64(b)) => a.to_bits() == b.to_bits(),
            (Self::I64(_), Self::F64(_)) | (Self::F64(_), Self::I64(_)) => false,
        }
    }

    /// This value as an `f64` for visual quantities; lossy past 2^53, so no
    /// write-back path uses it.
    pub(crate) const fn widen(self) -> f64 {
        match self {
            Self::I64(v) => v as f64,
            Self::F64(v) => v,
        }
    }
}

/// How a value widget stores a number: snapped to the step grid if one is set, clamped into
/// `lo..=hi`, a float target rounded to `decimals` fractional digits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoreRule {
    lo: f64,
    hi: f64,
    decimals: usize,
    /// Where the step grid starts: the widget's `min`, or zero while `min` is unbounded.
    anchor: f64,
    step: Option<f64>,
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
            anchor: if min.is_finite() { min } else { 0.0 },
            step: None,
        }
    }

    /// The rule with every stored value on the grid of `step` from `min`, the widget's
    /// [`step`](crate::Slider::step); `None` keeps the values continuous.
    pub(crate) const fn step(mut self, step: Option<f64>) -> Self {
        self.step = step;
        self
    }

    /// `value` on the grid point nearest it inside the bounds, so a span that is no multiple of
    /// the step ends on its last grid value (`0..=10` by 4 takes 10 to 8); no grid passes the
    /// value through. The point is rounded to the digits the step and the anchor hold, so the
    /// third step of 0.1 is 0.3, not `3.0 * 0.1`, as Blink's `StepRange` steps in decimal.
    fn snapped(self, value: f64) -> f64 {
        let Some(step) = self.step else {
            return value;
        };
        let first = self.steps_to(self.lo, step, f64::ceil);
        let last = self.steps_to(self.hi, step, f64::floor);
        let k = ((value - self.anchor) / step).round().clamp(first, last);
        let digits = decimal_digits(step).max(decimal_digits(self.anchor));
        round_to_decimals(self.anchor + k * step, digits)
    }

    /// The whole count of `step`s from the anchor to `bound`, rounded by `inward` unless the
    /// count is whole. A span that is a whole number of steps divides a few ulps off
    /// (`0.3 / 0.1` is `2.9999999999999996`); the span and the product each round by half an ulp
    /// of the span, so a product that close to it is the whole count. An unbounded side counts
    /// infinite steps.
    fn steps_to(self, bound: f64, step: f64, inward: fn(f64) -> f64) -> f64 {
        let span = bound - self.anchor;
        let count = span / step;
        if (count.round() * step - span).abs() <= 2.0 * f64::EPSILON * span.abs() {
            count.round()
        } else {
            inward(count)
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

impl BoundNum<'_> {
    /// The value a gesture moves from. A non-finite float reads as `0.0`, the neutral, so a
    /// scrub or a step from a NaN or infinite binding lands on a number inside the bounds; a NaN
    /// anchor in an unbounded range would clamp to `-inf`.
    pub(crate) fn read(&self) -> Num {
        match self.stored() {
            Num::F64(v) if !v.is_finite() => Num::F64(0.0),
            num => num,
        }
    }

    /// The stored value bit for bit, which [`Self::restore`] writes back.
    pub(crate) fn stored(&self) -> Num {
        match self {
            BoundNum::I32(v) => Num::I64(i64::from(**v)),
            BoundNum::I64(v) => Num::I64(**v),
            BoundNum::F32(v) => Num::F64(f64::from(**v)),
            BoundNum::F64(v) => Num::F64(**v),
        }
    }

    /// Commit a scrubbed value: `anchor` moved by `offset`, stored under `rule`. The
    /// integer target adds the nearest whole step; the float target snaps to
    /// `rule.decimals` (`1.98457…` at 3 → `1.985`). Under a step grid the sum lands on the grid,
    /// in `f64`, and the grid point is the value: no decimals round it off the grid (a step of
    /// 0.125 at 2 decimals keeps 0.125). Returns whether the stored value changed.
    pub(crate) fn commit_drag(&mut self, anchor: Num, offset: f64, rule: StoreRule) -> bool {
        let (anchor, offset) = match rule.step {
            Some(_) => (Num::F64(rule.snapped(anchor.widen() + offset)), 0.0),
            None => (anchor, offset),
        };
        let whole = || match anchor {
            Num::I64(a) => a.saturating_add(whole_step(offset)),
            Num::F64(a) => whole_step(a + offset),
        };
        let snapped = || match rule.step {
            Some(_) => anchor.widen(),
            None => round_to_decimals(anchor.widen() + offset, rule.decimals),
        };
        match self {
            BoundNum::I32(v) => store_int(*v, whole(), rule, I32_RANGE),
            BoundNum::I64(v) => store_int(*v, whole(), rule, I64_RANGE),
            BoundNum::F32(v) => store_f32(v, snapped(), rule),
            BoundNum::F64(v) => store_f64(v, snapped(), rule),
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
            BoundNum::I32(_) | BoundNum::I64(_) => 1.0,
            BoundNum::F32(_) | BoundNum::F64(_) => 1.0 / decimal_shift(decimals),
        }
    }

    /// Write back `value` exactly, undoing a canceled edit. Returns whether it
    /// changed.
    pub(crate) fn restore(&mut self, value: Num) -> bool {
        let changed = !self.stored().same_bits(value);
        match (self, value) {
            (BoundNum::I32(v), Num::I64(n)) => {
                **v = i32::try_from(n).expect("the restored value came from this i32");
            }
            (BoundNum::I64(v), Num::I64(n)) => **v = n,
            // Exact: the value widened from this `f32`.
            (BoundNum::F32(v), Num::F64(n)) => **v = n as f32,
            (BoundNum::F64(v), Num::F64(n)) => **v = n,
            (_, value) => unreachable!("{value:?} restored into a binding of the other type"),
        }
        changed
    }

    /// The value at `decimals` places, then `suffix`, interned for this frame.
    pub(crate) fn label(&self, ui: &mut Ui, decimals: usize, suffix: &str) -> InternedStr {
        match self {
            BoundNum::I32(v) => ui.fmt(format_args!("{}{suffix}", **v)),
            BoundNum::I64(v) => ui.fmt(format_args!("{}{suffix}", **v)),
            BoundNum::F32(v) => ui.fmt(format_args!("{:.*}{suffix}", decimals, **v)),
            BoundNum::F64(v) => ui.fmt(format_args!("{:.*}{suffix}", decimals, **v)),
        }
    }

    pub(crate) fn edit_string(&self) -> String {
        match self {
            BoundNum::I32(v) => v.to_string(),
            BoundNum::I64(v) => v.to_string(),
            BoundNum::F32(v) => format!("{:?}", **v),
            BoundNum::F64(v) => format!("{:?}", **v),
        }
    }

    /// Parse `text` and write it clamped into `rule`'s bounds, keeping the typed digits past
    /// `rule`'s decimals unless a step grid takes the value to its nearest point. Unparseable or
    /// non-finite text leaves the value alone, since a committed NaN would poison every later
    /// scrub.
    pub(crate) fn parse_from(&mut self, text: &str, rule: StoreRule) -> bool {
        if rule.step.is_some() {
            let parsed = match self {
                BoundNum::I32(_) | BoundNum::I64(_) => text.parse::<i64>().ok().map(|n| n as f64),
                BoundNum::F32(_) | BoundNum::F64(_) => {
                    text.parse::<f64>().ok().filter(|n| n.is_finite())
                }
            };
            return parsed.is_some_and(|n| self.commit_value(n, rule));
        }
        match self {
            // Parsed as `i64`: widening through `f64` would lose low bits past 2^53.
            BoundNum::I32(v) => text
                .parse::<i64>()
                .is_ok_and(|n| store_int(*v, n, rule, I32_RANGE)),
            BoundNum::I64(v) => text
                .parse::<i64>()
                .is_ok_and(|n| store_int(*v, n, rule, I64_RANGE)),
            BoundNum::F32(v) => match text.parse::<f64>() {
                Ok(n) if n.is_finite() => store_f32(v, n, rule),
                _ => false,
            },
            BoundNum::F64(v) => match text.parse::<f64>() {
                Ok(n) if n.is_finite() => store_f64(v, n, rule),
                _ => false,
            },
        }
    }
}

/// Store `next` clamped into `rule`'s bounds and the slot's own `range`, answering whether the
/// value moved. The bounds close in to the integers inside them (`0.5..=10.0` stores no 0, where
/// a truncating cast let it through); a range holding no integer clamps to the two either side.
fn store_int<T>(slot: &mut T, next: i64, rule: StoreRule, range: RangeInclusive<i64>) -> bool
where
    T: Copy + PartialEq + TryFrom<i64>,
{
    // The saturating casts pin an unbounded rule to the `i64` range.
    let lo = (rule.lo.ceil() as i64).max(*range.start());
    let hi = (rule.hi.floor() as i64).min(*range.end());
    let next = next.clamp(lo.min(hi), hi.max(lo));
    let next = T::try_from(next)
        .ok()
        .expect("clamped into the slot's range");
    let changed = *slot != next;
    *slot = next;
    changed
}

/// The values an `i32` slot holds, widened.
const I32_RANGE: RangeInclusive<i64> = i32::MIN as i64..=i32::MAX as i64;

/// The values an `i64` slot holds.
const I64_RANGE: RangeInclusive<i64> = i64::MIN..=i64::MAX;

/// The float half of [`store_int`]. `+ 0.0` turns `-0.0` into `+0.0`, which the clamp may
/// pass through and print as "-0.00"; the comparison is bit-exact.
fn store_f64(slot: &mut f64, next: f64, rule: StoreRule) -> bool {
    let next = rule.clamp(next) + 0.0;
    let changed = slot.to_bits() != next.to_bits();
    *slot = next;
    changed
}

/// [`store_f64`] for an `f32` slot: clamped and snapped in `f64`, then rounded to the nearest
/// `f32`, as the slot holds no more.
fn store_f32(slot: &mut f32, next: f64, rule: StoreRule) -> bool {
    let next = (rule.clamp(next) + 0.0) as f32;
    let changed = slot.to_bits() != next.to_bits();
    *slot = next;
    changed
}

impl<'a> From<&'a mut i32> for BoundNum<'a> {
    fn from(v: &'a mut i32) -> Self {
        BoundNum::I32(v)
    }
}

impl<'a> From<&'a mut f32> for BoundNum<'a> {
    fn from(v: &'a mut f32) -> Self {
        BoundNum::F32(v)
    }
}

impl<'a> From<&'a mut i64> for BoundNum<'a> {
    fn from(v: &'a mut i64) -> Self {
        BoundNum::I64(v)
    }
}

impl<'a> From<&'a mut f64> for BoundNum<'a> {
    fn from(v: &'a mut f64) -> Self {
        BoundNum::F64(v)
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

/// The fewest decimals that write `v` back exactly, at most [`MAX_DECIMALS`]: 1 for 0.1, 3 for
/// 0.125, 0 for a whole number, and the most for a value no decimals end, as 1/3.
fn decimal_digits(v: f64) -> usize {
    (0..MAX_DECIMALS)
        .find(|&decimals| round_to_decimals(v, decimals) == v)
        .unwrap_or(MAX_DECIMALS)
}

/// `10^decimals`; `decimals` is at most [`MAX_DECIMALS`].
fn decimal_shift(decimals: usize) -> f64 {
    debug_assert!(
        decimals <= MAX_DECIMALS,
        "decimals is coerced at the setter"
    );
    10f64.libm_powi(decimals as i32)
}

#[cfg(test)]
mod tests;
