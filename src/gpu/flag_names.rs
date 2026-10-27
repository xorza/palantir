//! The words Palantir's graphics flag sets display: the graphics API's own flag names.

use std::fmt;
use std::fmt::Formatter;

/// The set flags of a graphics-API bitfield, by name and joined by `, `; an empty set writes nothing. Takes an iterator as the callers' flag types are unrelated.
pub(crate) fn write<T>(
    f: &mut Formatter<'_>,
    names: impl Iterator<Item = (&'static str, T)>,
) -> fmt::Result {
    for (i, (name, _)) in names.enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        f.write_str(name)?;
    }
    Ok(())
}

/// Mirrors a Palantir flag set onto the graphics-API bitfield of the same meaning: `to_wgpu`,
/// `from_wgpu_truncate`, which drops what Palantir does not require, and a `Display` of the API's
/// names. A macro, as the conversions are `const` and a generic over the two flag types cannot be.
macro_rules! wgpu_mirror {
    ($name:ident => $api:ty { $($flag:ident => $api_flag:expr),+ $(,)? }) => {
        impl $name {
            /// Each flag beside the API flag it stands for; every declared flag appears once.
            const WGPU: &[(Self, $api)] = &[$((Self::$flag, $api_flag)),+];

            pub(crate) const fn to_wgpu(self) -> $api {
                let mut api = <$api>::empty();
                let mut i = 0;
                while i < Self::WGPU.len() {
                    let (flag, api_flag) = Self::WGPU[i];
                    if self.contains(flag) {
                        api = api.union(api_flag);
                    }
                    i += 1;
                }
                api
            }

            /// The flags whose API flags `api` holds; API flags Palantir does not require drop.
            pub(crate) const fn from_wgpu_truncate(api: $api) -> Self {
                let mut set = Self::NONE;
                let mut i = 0;
                while i < Self::WGPU.len() {
                    let (flag, api_flag) = Self::WGPU[i];
                    if api.contains(api_flag) {
                        set.insert(flag);
                    }
                    i += 1;
                }
                set
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                $crate::gpu::flag_names::write(f, self.to_wgpu().iter_names())
            }
        }
    };
}
pub(crate) use wgpu_mirror;

#[cfg(test)]
mod tests {
    use std::fmt;

    use crate::gpu::flag_names;

    fn names(usages: wgpu::TextureUsages) -> String {
        fmt::from_fn(|f| flag_names::write(f, usages.iter_names())).to_string()
    }

    #[test]
    fn writes_the_names_alone() {
        assert_eq!(names(wgpu::TextureUsages::COPY_SRC), "COPY_SRC");
        assert_eq!(
            names(wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC),
            "COPY_SRC, COPY_DST",
            "in the API's declaration order, whatever order they were joined in"
        );
        assert_eq!(
            names(wgpu::TextureUsages::empty()),
            "",
            "nothing missing reads as nothing, not as a bare wrapper"
        );
    }
}
