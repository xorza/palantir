use crate::gpu::spectrum_tables::{SPECTRUM_SAMPLES, SpectrumTables, cie_xyz};

/// Wyman's fit holds the CIE 1931 table's peaks within its stated error: ȳ at 555 nm is 1.000
/// in the table, x̄ at 600 nm 1.0622 and z̄ at 445 nm 1.7826. The weights sum to 1 in each
/// channel within an `f32` rounding a cell; the red end holds the most of x̄'s weight, the
/// violet end of z̄'s, and green the most of ȳ's. The running integrals are the sums of the
/// cells before: the area of the weight, and its moment about `t = 0` at each cell's middle.
#[test]
fn the_spectrum_sums_to_one_with_each_channel_where_its_lobe_peaks() {
    let [_, y, _] = cie_xyz(555.0);
    assert!((y - 1.0).abs() < 0.03, "{y}");
    let [x, _, _] = cie_xyz(600.0);
    assert!((x - 1.0622).abs() < 0.03, "{x}");
    let [_, _, z] = cie_xyz(445.0);
    assert!((z - 1.7826).abs() < 0.05, "{z}");

    let tables = SpectrumTables::new();
    let w = &tables.weights;
    let rounding = SPECTRUM_SAMPLES as f64 * f64::from(f32::EPSILON);
    for c in 0..3 {
        let sum: f64 = w.iter().map(|w| f64::from(w[c])).sum();
        assert!((sum - 1.0).abs() < rounding, "channel {c}: {sum}");
    }
    let peak = |c: usize| {
        (0..SPECTRUM_SAMPLES)
            .max_by(|&a, &b| w[a][c].total_cmp(&w[b][c]))
            .unwrap()
    };
    assert!(
        peak(0) < peak(1) && peak(1) < peak(2),
        "{}, {}, {}",
        peak(0),
        peak(1),
        peak(2)
    );

    assert_eq!(tables.cumulative[0], [0.0; 3]);
    assert_eq!(tables.moments[0], [0.0; 3]);
    let n = SPECTRUM_SAMPLES as f64;
    let (mut area, mut moment) = ([0.0_f64; 3], [0.0_f64; 3]);
    let starts = tables.cumulative.iter().zip(&tables.moments);
    for (j, (weight, (got_area, got_moment))) in w.iter().zip(starts).enumerate() {
        let sums = area.iter().zip(&moment);
        for ((want_area, want_moment), (got_area, got_moment)) in
            sums.zip(got_area.iter().zip(got_moment))
        {
            assert!((f64::from(*got_area) - want_area).abs() < rounding, "{j}");
            assert!(
                (f64::from(*got_moment) - want_moment).abs() < rounding,
                "{j}"
            );
        }
        let middle = (j as f64 + 0.5) / n;
        for ((area, moment), w) in area.iter_mut().zip(&mut moment).zip(weight) {
            *area += f64::from(*w);
            *moment += f64::from(*w) * middle;
        }
    }
}
