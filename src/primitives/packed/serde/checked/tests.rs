use crate::primitives::math::domain;
use crate::primitives::packed::serde::checked;
use glam::Vec2;

/// Run one validator over `text` read as RON.
fn check<T>(
    validator: fn(&mut ron::Deserializer<'static>) -> Result<T, ron::Error>,
    text: &'static str,
) -> Result<T, String> {
    let mut deserializer = ron::Deserializer::from_str(text).expect("RON");
    validator(&mut deserializer).map_err(|error| error.to_string())
}

/// Each scalar rule at its ends: the last value it takes, the first it refuses, and the non-finite values every rule refuses, stating the rule it refused under. A length or an offset in a file stops at 8192; an angle and a positive rate have no such bound.
#[test]
fn each_scalar_rule_holds_its_ends() {
    type Rule = fn(&mut ron::Deserializer<'static>) -> Result<f32, ron::Error>;
    let rows: [(&str, Rule, &[&'static str], &[&'static str]); 5] = [
        (
            domain::FILE_LENGTH_RULE,
            |d| checked::length(d),
            &["0.0", "8192.0"],
            &["-1.0", "8192.001", "1e30", "inf", "NaN"],
        ),
        (
            domain::POSITIVE_RULE,
            |d| checked::positive(d),
            &["1e-30"],
            &["0.0", "-1.0", "inf"],
        ),
        (
            domain::FRACTION_RULE,
            |d| checked::fraction(d),
            &["0.0", "1.0"],
            &["-0.5", "1.5", "NaN"],
        ),
        (
            domain::FILE_OFFSET_RULE,
            |d| checked::offset(d),
            &["-8192.0", "8192.0"],
            &["-8192.001", "8192.001", "inf", "-inf", "NaN"],
        ),
        (
            domain::ANGLE_RULE,
            |d| checked::angle(d),
            &["-1e30", "1e30"],
            &["inf", "-inf", "NaN"],
        ),
    ];
    for (rule, validator, takes, refuses) in rows {
        for &text in takes {
            assert!(check(validator, text).is_ok(), "{rule}: takes {text}");
        }
        for &text in refuses {
            let error = check(validator, text).expect_err(text);
            assert!(error.contains(rule), "{rule}: on {text}: {error}");
        }
    }
}

/// The vector, spacing and corner rules check every axis, edge or radius: one bad one refuses the value.
#[test]
fn vector_rules_check_every_axis() {
    assert_eq!(
        check(|d| checked::length2(d), "(1.0, 2.0)"),
        Ok(Vec2::new(1.0, 2.0))
    );
    assert!(check(|d| checked::length2(d), "(1.0, -2.0)").is_err());
    assert_eq!(
        check(|d| checked::offset2(d), "(-1.0, 2.0)"),
        Ok(Vec2::new(-1.0, 2.0))
    );
    assert!(check(|d| checked::offset2(d), "(inf, 2.0)").is_err());
    assert!(check(|d| checked::length2(d), "(1.0, 8193.0)").is_err());
    assert!(check(|d| checked::offset2(d), "(-8193.0, 0.0)").is_err());
    assert!(check(|d| checked::padding(d), "8192.0").is_ok());
    // A spacing is checked as packed: 8193 rounds to the f16 8192, and 8200 is the next lane value.
    let error = check(|d| checked::padding(d), "(1.0, 8200.0)").expect_err("a long edge");
    assert!(error.contains(domain::FILE_LENGTH_RULE), "{error}");
    let error = check(|d| checked::margin(d), "-8200.0").expect_err("a long edge");
    assert!(error.contains(domain::FILE_OFFSET_RULE), "{error}");
    assert!(check(|d| checked::corners(d), "8192.0").is_ok());
    let error = check(|d| checked::corners(d), "(0.0, 8200.0)").expect_err("a long radius");
    assert!(error.contains(domain::FILE_LENGTH_RULE), "{error}");
    assert!(
        check(
            |d| checked::offset_points3(d),
            "((0.0, 0.0), (1.0, 1.0), (2.0, 0.0))"
        )
        .is_ok()
    );
    let error = check(
        |d| checked::offset_points3(d),
        "((0.0, 0.0), (NaN, 1.0), (2.0, 0.0))",
    )
    .expect_err("a NaN point");
    assert!(error.contains(domain::FILE_OFFSET_RULE), "{error}");
}
