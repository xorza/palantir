//! The two paths things travel in the reel: the cursor's reach and the camera's glide.
#![expect(
    clippy::disallowed_methods,
    reason = "a demo of the crate, whose frames owe no two systems the same bits"
)]

use std::ops::{Add, Div, Mul, Sub};

/// Minimum jerk (Flash and Hogan 1985): the share of a reach covered at `t` of its time,
/// `10t³ − 15t⁴ + 6t⁵`, which minimizes the integrated squared jerk and starts and ends at rest,
/// as a human reach does.
pub(crate) fn min_jerk(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (10.0 + t * (6.0 * t - 15.0))
}

/// [`min_jerk`]'s speed in shares a unit of time, `30t²(1 − t)²`.
#[cfg(test)]
fn min_jerk_speed(t: f64) -> f64 {
    30.0 * t * t * (1.0 - t) * (1.0 - t)
}

/// A canvas point in `f64`: a glide's cosh and tanh lose a pixel in `f32` across a canvas of
/// 6,000 px, and glam here has no `f64` vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Point {
    pub(crate) const ZERO: Self = Self::new(0.0, 0.0);

    pub(crate) const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub(crate) fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
}

impl Add for Point {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, k: f64) -> Self {
        Self::new(self.x * k, self.y * k)
    }
}

impl Div<f64> for Point {
    type Output = Self;
    fn div(self, k: f64) -> Self {
        Self::new(self.x / k, self.y / k)
    }
}

/// A camera's view: the canvas point at its center and the canvas width it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) center: Point,
    pub(crate) width: f64,
}

/// The optimal pan-zoom path between two views, van Wijk and Nuij's ("Smooth and efficient
/// zooming and panning", InfoVis 2003): the camera zooms out as it pans and in as it lands,
/// along the path a viewer reads at the least perceived speed. Its own parameter runs at a
/// constant perceived speed, which starts and stops with a jolt, so [`Self::at`] spends it along
/// [`min_jerk`], as Reach and North smooth the ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glide {
    from: View,
    /// The unit direction from the first center to the last, zero for a pure zoom.
    dir: Point,
    /// The path's length in its own parameter.
    length: f64,
    shape: Shape,
}

/// How the width and the pan move along the path.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// Centers apart: the cosh curve, its `r0`.
    Pan { r0: f64 },
    /// One center: the width moves exponentially, up or down.
    Zoom { sign: f64 },
}

/// The trade between zooming and panning, √2, the value van Wijk and Nuij found people prefer.
const RHO: f64 = std::f64::consts::SQRT_2;

impl Glide {
    pub(crate) fn new(from: View, to: View) -> Self {
        let delta = to.center - from.center;
        let u1 = delta.length();
        let (w0, w1) = (from.width, to.width);
        if u1 < 1e-9 * w0.max(w1) {
            return Self {
                from,
                dir: Point::ZERO,
                length: (w1 / w0).ln().abs() / RHO,
                shape: Shape::Zoom {
                    sign: (w1 - w0).signum(),
                },
            };
        }
        let rho2 = RHO * RHO;
        let b = |w: f64, sign: f64| {
            (w1 * w1 - w0 * w0 + sign * rho2 * rho2 * u1 * u1) / (2.0 * w * rho2 * u1)
        };
        let r = |b: f64| (-b + (b * b + 1.0).sqrt()).ln();
        let (r0, r1) = (r(b(w0, 1.0)), r(b(w1, -1.0)));
        Self {
            from,
            dir: delta / u1,
            length: (r1 - r0) / RHO,
            shape: Shape::Pan { r0 },
        }
    }

    /// The view at `t`, `0..=1` of the glide's time.
    pub(crate) fn at(&self, t: f64) -> View {
        let s = self.length * min_jerk(t);
        let View { center, width: w0 } = self.from;
        match self.shape {
            Shape::Zoom { sign } => View {
                center,
                width: w0 * (sign * RHO * s).exp(),
            },
            Shape::Pan { r0 } => {
                let rho2 = RHO * RHO;
                let u = w0 / rho2 * r0.cosh() * (RHO * s + r0).tanh() - w0 / rho2 * r0.sinh();
                View {
                    center: center + self.dir * u,
                    width: w0 * r0.cosh() / (RHO * s + r0).cosh(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::motion::{Glide, Point, View, min_jerk, min_jerk_speed};

    /// A reach starts at 0 and ends at 1 at rest, and its peak speed, at the midpoint, is 15/8
    /// of its mean: `30 · (½)² · (½)² = 30/16`, over a mean of 1, as Flash and Hogan's model
    /// predicts.
    #[test]
    fn a_minimum_jerk_reach_ends_at_rest_with_a_peak_of_15_8() {
        assert_eq!(min_jerk(0.0), 0.0);
        assert_eq!(min_jerk(1.0), 1.0);
        assert_eq!(min_jerk(0.5), 0.5);
        assert_eq!(min_jerk_speed(0.0), 0.0);
        assert_eq!(min_jerk_speed(1.0), 0.0);
        assert_eq!(min_jerk_speed(0.5), 15.0 / 8.0);
        // The speed is the derivative: a central difference agrees to its own error.
        let h = 1e-6;
        for t in [0.1, 0.3, 0.7] {
            let slope = (min_jerk(t + h) - min_jerk(t - h)) / (2.0 * h);
            assert!((slope - min_jerk_speed(t)).abs() < 1e-8, "{t}");
        }
    }

    /// A glide starts on its first view and lands on its last; a pan between two views of one
    /// width zooms out on the way, and a zoom at one center stays there.
    #[test]
    fn a_glide_runs_between_its_views_and_zooms_out_to_pan() {
        let a = View {
            center: Point::new(640.0, 360.0),
            width: 1280.0,
        };
        let b = View {
            center: Point::new(640.0 + 4.0 * 1280.0, 360.0),
            width: 1280.0,
        };
        let glide = Glide::new(a, b);
        let close = |x: View, y: View| {
            (x.center - y.center).length() < 1e-6 && (x.width - y.width).abs() < 1e-6
        };
        assert!(close(glide.at(0.0), a));
        assert!(close(glide.at(1.0), b));
        let mid = glide.at(0.5);
        assert!(
            (mid.center.x - 640.0 - 2.0 * 1280.0).abs() < 1e-6,
            "{mid:?}"
        );
        assert!(mid.width > 1280.0, "it zooms out to pan: {mid:?}");

        let zoom = Glide::new(a, View { width: 320.0, ..a });
        assert!(close(zoom.at(1.0), View { width: 320.0, ..a }));
        assert_eq!(zoom.at(0.5).center, a.center);
        assert!(
            (zoom.at(0.5).width - 640.0).abs() < 1e-9,
            "the geometric mean"
        );
    }
}
