use super::num::Num;
use super::size::Size;
use glam::Vec2;

/// Per-corner radii. `Vec2`/`Size` map to (top, bottom) pairs; `f32` is uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

// Serialize Corners compactly:
// - all four equal     → bare scalar `4.0`
// - tl=tr, br=bl       → 2-element array `[top, bottom]` (CSS-style shorthand)
// - otherwise          → 4-element array `[tl, tr, br, bl]`
// Deserialize accepts all three forms plus the original struct shape
// `{ tl, tr, br, bl }` for backward-compat with hand-written configs.
impl serde::Serialize for Corners {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        if self.tl == self.tr && self.tr == self.br && self.br == self.bl {
            return s.serialize_f32(self.tl);
        }
        if self.tl == self.tr && self.br == self.bl {
            let mut seq = s.serialize_seq(Some(2))?;
            seq.serialize_element(&self.tl)?;
            seq.serialize_element(&self.br)?;
            return seq.end();
        }
        let mut seq = s.serialize_seq(Some(4))?;
        seq.serialize_element(&self.tl)?;
        seq.serialize_element(&self.tr)?;
        seq.serialize_element(&self.br)?;
        seq.serialize_element(&self.bl)?;
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for Corners {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Corners;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number, a [top, bottom] or [tl, tr, br, bl] array, or a {tl, tr, br, bl} table")
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Corners, E> {
                Ok(Corners::all(v as f32))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Corners, A::Error> {
                let v0: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let Some(v1) = a.next_element::<f32>()? else {
                    return Ok(Corners::all(v0));
                };
                let Some(v2) = a.next_element::<f32>()? else {
                    return Ok(Corners {
                        tl: v0,
                        tr: v0,
                        br: v1,
                        bl: v1,
                    });
                };
                let v3: f32 = a
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                Ok(Corners {
                    tl: v0,
                    tr: v1,
                    br: v2,
                    bl: v3,
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Corners, A::Error> {
                let mut tl = None;
                let mut tr = None;
                let mut br = None;
                let mut bl = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "tl" => tl = Some(m.next_value()?),
                        "tr" => tr = Some(m.next_value()?),
                        "br" => br = Some(m.next_value()?),
                        "bl" => bl = Some(m.next_value()?),
                        other => {
                            return Err(serde::de::Error::unknown_field(
                                other,
                                &["tl", "tr", "br", "bl"],
                            ));
                        }
                    }
                }
                Ok(Corners {
                    tl: tl.unwrap_or(0.0),
                    tr: tr.unwrap_or(0.0),
                    br: br.unwrap_or(0.0),
                    bl: bl.unwrap_or(0.0),
                })
            }
        }
        d.deserialize_any(V)
    }
}

impl Corners {
    pub const ZERO: Self = Self {
        tl: 0.0,
        tr: 0.0,
        br: 0.0,
        bl: 0.0,
    };
    pub const fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }
    pub const fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self { tl, tr, br, bl }
    }
    pub const fn scaled_by(&self, scale: f32) -> Self {
        Self {
            tl: self.tl * scale,
            tr: self.tr * scale,
            br: self.br * scale,
            bl: self.bl * scale,
        }
    }
}

impl<T: Num> From<T> for Corners {
    fn from(r: T) -> Self {
        Self::all(r.as_f32())
    }
}

impl From<Vec2> for Corners {
    fn from(v: Vec2) -> Self {
        Self {
            tl: v.x,
            tr: v.x,
            br: v.y,
            bl: v.y,
        }
    }
}

impl From<Size> for Corners {
    fn from(s: Size) -> Self {
        Self {
            tl: s.w,
            tr: s.w,
            br: s.h,
            bl: s.h,
        }
    }
}
