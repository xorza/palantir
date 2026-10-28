use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;

/// σ from a quarter pixel to 400 px, and either side of every level boundary.
fn sigmas() -> Vec<f32> {
    let mut sigmas: Vec<f32> = (1..=1600).map(|i| i as f32 * 0.25).collect();
    for boundary in [4.0f32, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0] {
        sigmas.extend([boundary.next_down(), boundary, boundary.next_up()]);
    }
    sigmas
}

/// The variance of the residual's taps, in texel².
fn taps_variance(plan: BlurPlan) -> f64 {
    let taps = plan.taps();
    let total = f64::from(taps[0]) + 2.0 * taps[1..].iter().map(|&w| f64::from(w)).sum::<f64>();
    let moment: f64 = taps
        .iter()
        .enumerate()
        .map(|(k, &w)| 2.0 * f64::from(w) * (k * k) as f64)
        .sum();
    moment / total
}

/// Each stage's fixed variance plus the residual's, taken from its actual taps, is σ²: the taps
/// are `f32`, so the sum holds to their rounding (relative `1e-6`).
#[test]
fn stage_variances_sum_to_sigma_squared() {
    for sigma in sigmas() {
        let plan = BlurPlan::new(sigma);
        let texel = f64::from(plan.lattice());
        let total = BlurPlan::fixed_variance(plan.levels()) + taps_variance(plan) * texel * texel;
        let want = f64::from(sigma).powi(2);
        assert!(
            (total - want).abs() <= 1e-6 * want.max(1e-6),
            "σ {sigma}: stages sum to {total}, want {want}"
        );
    }
}

/// Each level holds σ up to four of its texels, so a level boundary sits at σ = 4, 8, 16 … px.
#[test]
fn levels_step_at_each_boundary() {
    for (sigma, levels) in [
        (0.0, 0),
        (4.0, 0),
        (4.01, 1),
        (8.0, 1),
        (8.01, 2),
        (16.0, 2),
        (16.01, 3),
        (40.0, 4),
        (160.0, 6),
        (BlurPlan::MAX_SIGMA, BlurPlan::MAX_LEVELS),
    ] {
        assert_eq!(BlurPlan::new(sigma).levels(), levels, "σ {sigma}");
    }
    // 16384 = 4 · 2¹² px is the last level's widest; past it, and at infinity, σ coerces to it.
    assert_eq!(BlurPlan::MAX_SIGMA, 16384.0);
    let widest = BlurPlan::new(BlurPlan::MAX_SIGMA);
    for sigma in [BlurPlan::MAX_SIGMA.next_up(), 1.0e9, f32::INFINITY] {
        assert_eq!(BlurPlan::new(sigma), widest, "σ {sigma}");
    }
}

/// The residual stays above 3.4 texel² on every blurred level: σ there exceeds two texels, 4
/// texel², and the tents and the B-spline take at most `0.75 · (1 + 1/4 + …) + 1/3 < 0.59` of it.
#[test]
fn the_residual_is_never_spent() {
    for sigma in sigmas() {
        let plan = BlurPlan::new(sigma);
        if plan.levels() == 0 {
            continue;
        }
        let floor = 3.4;
        let residual = taps_variance(plan);
        assert!(residual >= floor, "σ {sigma}: residual {residual} texel²");
    }
}

/// An impulse at pixel `at` of `len` pixels, through the whole reference blur: the composite's
/// read at every pixel.
fn response(plan: BlurPlan, len: usize, at: usize) -> Vec<f64> {
    let mut pixels = vec![0.0; len];
    pixels[at] = 1.0;
    let level = plan.reference_residual(&plan.reference_level(&pixels));
    (0..len).map(|n| plan.reference_read(&level, n)).collect()
}

/// Sums, mean and variance of `response` about pixel `at`.
#[derive(Debug)]
struct Moments {
    total: f64,
    mean: f64,
    second: f64,
}

fn moments(response: &[f64], at: usize) -> Moments {
    let total: f64 = response.iter().sum();
    let offset = |n: usize| n as f64 - at as f64;
    let mean = response
        .iter()
        .enumerate()
        .map(|(n, w)| w * offset(n))
        .sum::<f64>()
        / total;
    let second = response
        .iter()
        .enumerate()
        .map(|(n, w)| w * offset(n).powi(2))
        .sum::<f64>()
        / total;
    Moments {
        total,
        mean,
        second,
    }
}

/// The whole blur, sampled at pixel centers, keeps an impulse's energy, its mean, and has
/// variance σ² about it at every phase of the lattice: each tent's two phases keep the mean and
/// add the same variance, and the cubic B-spline's samples at whole pixels of a `2^levels` lattice
/// carry its moments exactly. The tolerance is the `f32` taps' (relative `1e-6`).
#[test]
fn the_blur_has_variance_sigma_squared_at_every_phase() {
    for sigma in [0.5f32, 1.5, 4.0, 4.01, 6.0, 8.01, 20.0, 48.0, 100.0] {
        let plan = BlurPlan::new(sigma);
        let lattice = plan.lattice() as usize;
        let len = 64 * lattice + 4 * plan.reach() as usize;
        for phase in 0..lattice {
            let at = len / 2 / lattice * lattice + phase;
            let m = moments(&response(plan, len, at), at);
            let want = f64::from(sigma).powi(2);
            assert!(
                (m.total - 1.0).abs() < 1e-6,
                "σ {sigma} phase {phase}: energy {}",
                m.total
            );
            let second = m.second;
            assert!(
                (second - want).abs() <= 1e-6 * want,
                "σ {sigma} phase {phase}: second moment {second}, want {want}"
            );
            assert!(
                m.mean.abs() < 1e-9,
                "σ {sigma} phase {phase}: mean {}",
                m.mean
            );
        }
    }
}

/// A pixel's result reads nothing past `reach` of it, and something within one texel of it, so
/// the reach is tight.
#[test]
fn the_reach_is_the_blurs_support() {
    for sigma in [0.5f32, 1.5, 4.0, 4.01, 6.0, 8.01, 20.0, 48.0, 100.0] {
        let plan = BlurPlan::new(sigma);
        let lattice = plan.lattice() as usize;
        let reach = plan.reach() as usize;
        let len = 8 * lattice + 4 * reach;
        for phase in 0..lattice {
            let at = len / 2 / lattice * lattice + phase;
            let response = response(plan, len, at);
            let farthest = response
                .iter()
                .enumerate()
                .filter(|&(_, &w)| w != 0.0)
                .map(|(n, _)| n.abs_diff(at))
                .max()
                .unwrap();
            assert!(
                farthest <= reach,
                "σ {sigma} phase {phase}: reads {farthest} px, reach {reach}"
            );
            assert!(
                farthest + lattice >= reach,
                "σ {sigma} phase {phase}: reads {farthest} px, reach {reach} is loose"
            );
        }
    }
}

/// The blur is a Gaussian in variance and mean, and near one in shape: at every phase the stages'
/// own shapes leave the impulse response within 0.4 % of the Gaussian of σ's peak, under one
/// 8-bit step, and the plans either side of a level boundary as close to each other.
#[test]
fn the_blur_is_near_a_gaussian_and_continuous_across_levels() {
    let gaussian = |sigma: f64, d: f64| {
        (-0.5 * (d / sigma).powi(2)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())
    };
    let deviation = |a: &[f64], b: &[f64]| {
        let peak = b.iter().copied().fold(0.0, f64::max);
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
            / peak
    };
    for sigma in [1.5f32, 4.0, 4.01, 6.0, 8.01, 16.01, 20.0, 48.0] {
        let plan = BlurPlan::new(sigma);
        let lattice = plan.lattice() as usize;
        let len = 64 * lattice + 4 * plan.reach() as usize;
        for phase in 0..lattice {
            let at = len / 2 / lattice * lattice + phase;
            let got = response(plan, len, at);
            let want: Vec<f64> = (0..len)
                .map(|n| gaussian(f64::from(sigma), n as f64 - at as f64))
                .collect();
            let off = deviation(&got, &want);
            assert!(
                off < 0.004,
                "σ {sigma} phase {phase}: {:.2} % off a Gaussian",
                off * 100.0
            );
        }
    }
    for boundary in [4.0f32, 8.0, 16.0, 32.0] {
        let (below, above) = (
            BlurPlan::new(boundary.next_down()),
            BlurPlan::new(boundary.next_up()),
        );
        let len = 64 * above.lattice() as usize + 4 * above.reach() as usize;
        let at = len / 2;
        let off = deviation(&response(below, len, at), &response(above, len, at));
        assert!(
            off < 0.004,
            "σ {boundary}: the levels either side differ by {:.2} %",
            off * 100.0
        );
    }
}

/// A footprint snaps out to the last level's texels and stays inside the surface: at σ = 10 (two
/// levels, 4 px texels) px 5..43 covers texels 1..11, and one past the surface's 50 px stops at
/// its 13th texel, `ceil(50 / 4)`.
#[test]
fn a_region_snaps_out_and_stays_inside() {
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.lattice(), 4);
    let surface = UVec2::new(50, 50);
    assert_eq!(
        plan.region(URect::new(5, 5, 38, 38), surface),
        Some(URect::new(1, 1, 10, 10))
    );
    assert_eq!(
        plan.region(URect::new(40, 0, 30, 4), surface),
        Some(URect::new(10, 0, 3, 1))
    );
    assert_eq!(plan.region(URect::new(60, 60, 4, 4), surface), None);
}
