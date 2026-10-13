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
