//! [`ErrorChain`]: an error and every cause under it, for a report a person reads.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// An error, then each cause [`Error::source`] reaches under it on a line of its own, in the shape
/// of std's unstable `std::error::Report`: palantir's errors name their cause in `source` alone,
/// so printing an error by itself hides what failed under it.
///
/// ```
/// # use palantir::ErrorChain;
/// # fn report(error: &dyn std::error::Error) {
/// eprintln!("{}", ErrorChain(error));
/// # }
/// ```
///
/// A chain past [`Self::MAX_DEPTH`] causes ends with a line that says so, as a source that leads
/// back into the chain would print forever.
#[derive(Clone, Copy, Debug)]
pub struct ErrorChain<'a>(pub &'a dyn Error);

impl ErrorChain<'_> {
    /// The most causes printed under the error.
    pub const MAX_DEPTH: usize = 64;
}

impl Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        for _ in 0..Self::MAX_DEPTH {
            let Some(error) = cause else {
                return Ok(());
            };
            write!(f, "\ncaused by: {error}")?;
            cause = error.source();
        }
        if cause.is_some() {
            write!(f, "\ncaused by: … more past {} causes", Self::MAX_DEPTH)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::common::error_chain::ErrorChain;
    use std::error::Error;
    use std::fmt::{self, Display, Formatter};

    /// An error `depth` causes deep, each naming its level.
    #[derive(Debug)]
    struct Nested {
        level: usize,
        below: Option<Box<Self>>,
    }

    impl Nested {
        fn deep(depth: usize) -> Self {
            (1..=depth).fold(
                Self {
                    level: 0,
                    below: None,
                },
                |below, level| Self {
                    level,
                    below: Some(Box::new(below)),
                },
            )
        }
    }

    impl Display for Nested {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "level {}", self.level)
        }
    }

    impl Error for Nested {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.below
                .as_deref()
                .map(|below| -> &(dyn Error + 'static) { below })
        }
    }

    /// The error prints alone with no cause, then each cause on its own line, in order; a chain
    /// past the bound prints the bound's causes and a last line that says more were left.
    #[test]
    fn a_chain_prints_each_cause_on_its_own_line() {
        assert_eq!(ErrorChain(&Nested::deep(0)).to_string(), "level 0");
        assert_eq!(
            ErrorChain(&Nested::deep(2)).to_string(),
            "level 2\ncaused by: level 1\ncaused by: level 0"
        );
        let long = ErrorChain(&Nested::deep(ErrorChain::MAX_DEPTH + 3)).to_string();
        let lines: Vec<&str> = long.lines().collect();
        assert_eq!(lines.len(), ErrorChain::MAX_DEPTH + 2);
        assert_eq!(
            lines[1],
            format!("caused by: level {}", ErrorChain::MAX_DEPTH + 2)
        );
        assert_eq!(
            lines.last().copied(),
            Some(format!("caused by: … more past {} causes", ErrorChain::MAX_DEPTH).as_str())
        );
        let exact = ErrorChain(&Nested::deep(ErrorChain::MAX_DEPTH)).to_string();
        assert_eq!(
            exact.lines().count(),
            ErrorChain::MAX_DEPTH + 1,
            "no line for none left"
        );
    }

    /// Every exported error that wraps another prints each message once down its chain: a
    /// transparent case hands on its inner error's causes, not the inner error again, and one
    /// that adds context names its own failure above the inner one. Each chain's depth is pinned,
    /// so a wrapper that drops its cause fails too.
    #[test]
    fn no_exported_chain_prints_a_message_twice() {
        use crate::driver::error::{LocateError, PlayError, StepError};
        use crate::effect::effect_registry::EffectRegistry;
        use crate::effect::effect_source::EffectSource;
        use crate::gpu::error::{DeviceRequirementsError, GpuRequestError};
        use crate::icons::icon_table::IconTable;
        use crate::primitives::effect::effect_name::EffectName;

        let locate = || LocateError::OffScreen {
            locator: "Text(\"Save\")".to_owned(),
        };
        let requirements = || GpuRequestError::Requirements {
            source: DeviceRequirementsError::Limit {
                name: "max_texture_dimension_2d",
                required: 8192,
                available: 4096,
            },
        };
        let effect = EffectRegistry::default()
            .load(
                EffectName::<()>::named("broken").expect("the name fits"),
                &EffectSource::new("fn effect(".to_owned()),
            )
            .unwrap_err();
        let icons = IconTable::from_svgs([("broken", "<svg")]).unwrap_err();
        #[cfg_attr(
            not(feature = "winit"),
            expect(unused_mut, reason = "only the winit host adds a chain")
        )]
        let mut chains: Vec<(&str, Box<dyn Error>, usize)> = vec![
            (
                "locate, transparent",
                Box::new(PlayError::Locate(locate())),
                1,
            ),
            (
                "step over a locate",
                Box::new(StepError {
                    step: 2,
                    source: PlayError::Locate(locate()),
                }),
                2,
            ),
            ("requirements", Box::new(requirements()), 2),
            ("effect over its WGSL", Box::new(effect), 2),
            ("icon over its SVG", Box::new(icons), 2),
        ];
        #[cfg(feature = "winit")]
        {
            use crate::host::winit::error::WinitHostError;
            chains.push((
                "host over its device",
                Box::new(WinitHostError::DeviceLost {
                    source: requirements(),
                }),
                3,
            ));
        }
        for (label, error, depth) in &chains {
            let printed = ErrorChain(error.as_ref()).to_string();
            let lines: Vec<&str> = printed
                .lines()
                .map(|line| line.trim_start_matches("caused by: "))
                .collect();
            assert_eq!(lines.len(), *depth, "{label}: {printed}");
            for (idx, line) in lines.iter().enumerate() {
                assert!(
                    !lines[idx + 1..].contains(line),
                    "{label} prints {line:?} twice: {printed}"
                );
            }
        }
    }
}
