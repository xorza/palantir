use crate::display::scale_factor_is_valid;
use crate::display::system_scale::SystemScale;
use crate::display::system_scale::error::SystemScaleError;
use crate::display::user_scale::UserScale;

/// A scale from `MIN` to `MAX` stands as given, unclamped; outside them, or not a number, is
/// refused, and `TryFrom` names the refused factor. `ONE` is the default.
#[test]
fn a_scale_in_range_stands_and_the_rest_is_refused() {
    assert_eq!(SystemScale::MIN, 2e-4, "1e-4 / 0.5, exact as a halving");
    for factor in [1.0, 2.0, 1.5, 64.0, SystemScale::MIN, SystemScale::MAX] {
        assert_eq!(SystemScale::new(factor).map(SystemScale::get), Some(factor));
        assert_eq!(
            SystemScale::try_from(factor).map(SystemScale::get),
            Ok(factor)
        );
    }
    let outside = [
        0.0,
        1e-4,
        SystemScale::MIN.next_down(),
        SystemScale::MAX.next_up(),
        -1.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    for factor in outside {
        assert_eq!(SystemScale::new(factor), None, "{factor}");
        assert_eq!(
            SystemScale::try_from(factor),
            Err(SystemScaleError { found: factor }),
            "{factor}"
        );
    }
    assert!(SystemScale::try_from(f32::NAN).is_err());
    assert_eq!(SystemScale::default(), SystemScale::ONE);
    assert_eq!(
        SystemScaleError { found: 0.0 }.to_string(),
        "a system scale must be from 2e-4 to 1.1342745e38, not 0"
    );
}

/// The bounds keep the product valid under every user scale, which is why a frame only
/// debug-asserts it.
#[test]
fn every_user_scale_keeps_the_product_in_range() {
    for system in [SystemScale::MIN, SystemScale::MAX] {
        let system = SystemScale::new(system).unwrap();
        for user in [UserScale::MIN, UserScale::MAX] {
            let product = UserScale::new(user).unwrap().applied_to(system);
            assert!(scale_factor_is_valid(product), "{system:?} × {user}");
        }
    }
}

/// The windowed door keeps a usable platform scale and replaces one that names no density with
/// one, so a bad report never reaches a division.
#[cfg(feature = "winit")]
#[test]
fn a_platform_scale_stands_or_falls_back_to_one() {
    for (reported, kept) in [
        (2.0_f64, 2.0_f32),
        (1.5, 1.5),
        (0.0, 1.0),
        (-2.0, 1.0),
        (f64::NAN, 1.0),
        (1e-4, 1.0),
        (1e40, 1.0),
    ] {
        assert_eq!(
            SystemScale::from_platform(reported).get(),
            kept,
            "{reported}"
        );
    }
}
