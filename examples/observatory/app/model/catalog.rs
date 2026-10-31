//! [`Catalog`]: the generated sky, every object a seed makes, and the lines between its bright
//! stars.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use palantir::glam::Vec3;

/// An object's place in the [`Catalog`], its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ObjectId(pub(crate) u32);

/// What an object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ObjectKind {
    Star,
    Cluster,
    Nebula,
    Galaxy,
}

impl ObjectKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Star => "star",
            Self::Cluster => "cluster",
            Self::Nebula => "nebula",
            Self::Galaxy => "galaxy",
        }
    }
}

/// One object of the sky.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Object {
    pub(crate) name: String,
    pub(crate) kind: ObjectKind,
    /// Right ascension in radians, `0..TAU`.
    pub(crate) ra: f32,
    /// Declination in radians, `-FRAC_PI_2..=FRAC_PI_2`.
    pub(crate) dec: f32,
    /// Visual magnitude; brighter is lower.
    pub(crate) mag: f32,
    /// The B−V color index, which [`crate::app::model::sky::star_color`] reads.
    pub(crate) bv: f32,
    /// The observer's note, empty until written.
    pub(crate) note: String,
}

/// The generated sky.
#[derive(Debug)]
pub(crate) struct Catalog {
    objects: Vec<Object>,
    /// Pairs of bright stars joined as the example's asterisms.
    lines: Vec<[ObjectId; 2]>,
}

/// The stars the generator makes.
pub(crate) const STAR_COUNT: u32 = 4000;
/// The deep-sky objects the generator makes, after the stars.
pub(crate) const DEEP_SKY_COUNT: u32 = 60;
/// The seed of the example's one sky; the same each run, so a shot draws the same each run.
pub(crate) const SEED: u64 = 0x05ee_d0fa_57a1;
/// The share of stars drawn toward the band, the example's Milky Way.
const BAND_SHARE: f32 = 0.4;
/// The band's spread in galactic latitude, radians (one standard deviation, 8°).
const BAND_SIGMA: f32 = 8.0 * PI / 180.0;
/// The faintest and the brightest magnitude the generator draws.
const MAG_RANGE: (f32, f32) = (-1.0, 6.5);
/// The stars that join asterisms: those brighter than this.
const ASTERISM_MAG: f32 = 2.6;
/// The longest asterism line, radians (12°).
const ASTERISM_REACH: f32 = 12.0 * PI / 180.0;

impl Catalog {
    /// The sky `seed` makes: [`STAR_COUNT`] stars, then [`DEEP_SKY_COUNT`] deep-sky objects.
    pub(crate) fn generate(seed: u64) -> Self {
        let mut rng = SplitMix64(seed);
        let mut objects = Vec::with_capacity((STAR_COUNT + DEEP_SKY_COUNT) as usize);
        for idx in 0..STAR_COUNT {
            let (ra, dec) = if rng.unit() < BAND_SHARE {
                band_position(&mut rng)
            } else {
                sphere_position(&mut rng)
            };
            objects.push(Object {
                name: format!("OBS {:04}", idx + 1),
                kind: ObjectKind::Star,
                ra,
                dec,
                mag: magnitude(rng.unit()),
                // The mean of two uniforms peaks mid-range, where most stars sit.
                bv: -0.4 + 2.4 * (rng.unit() + rng.unit()) * 0.5,
                note: String::new(),
            });
        }
        let kinds = [ObjectKind::Cluster, ObjectKind::Nebula, ObjectKind::Galaxy];
        for idx in 0..DEEP_SKY_COUNT {
            let kind = kinds[idx as usize % kinds.len()];
            // Clusters and nebulae keep to the band, galaxies away from it, as in the sky.
            let (ra, dec) = if kind == ObjectKind::Galaxy {
                sphere_position(&mut rng)
            } else {
                band_position(&mut rng)
            };
            objects.push(Object {
                name: format!("DSO {:02}", idx + 1),
                kind,
                ra,
                dec,
                mag: 4.0 + 7.0 * rng.unit(),
                bv: 0.6,
                note: String::new(),
            });
        }
        let lines = asterisms(&objects);
        Self { objects, lines }
    }

    pub(crate) fn get(&self, id: ObjectId) -> &Object {
        &self.objects[id.0 as usize]
    }

    pub(crate) fn get_mut(&mut self, id: ObjectId) -> &mut Object {
        &mut self.objects[id.0 as usize]
    }

    pub(crate) const fn len(&self) -> usize {
        self.objects.len()
    }

    /// Every object with its id, in id order.
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (ObjectId, &Object)> {
        self.objects
            .iter()
            .enumerate()
            .map(|(idx, object)| (ObjectId(idx as u32), object))
    }

    pub(crate) fn lines(&self) -> &[[ObjectId; 2]] {
        &self.lines
    }
}

/// A point uniform on the sphere: the sine of the declination is uniform.
fn sphere_position(rng: &mut SplitMix64) -> (f32, f32) {
    let ra = TAU * rng.unit();
    let dec = (2.0 * rng.unit() - 1.0).asin();
    (ra, dec)
}

/// A point near the band: galactic longitude uniform, latitude normal around 0, turned into
/// equatorial coordinates.
fn band_position(rng: &mut SplitMix64) -> (f32, f32) {
    let l = TAU * rng.unit();
    let b = (BAND_SIGMA * rng.normal()).clamp(-FRAC_PI_2, FRAC_PI_2);
    galactic_to_equatorial(l, b)
}

/// Galactic `(l, b)` in radians to equatorial `(ra, dec)` (J2000): the IAU 1958 frame, whose north
/// pole sits at RA 192.85948°, Dec 27.12825°, with the celestial pole at galactic longitude
/// 122.93192° (Hipparcos, ESA SP-1200 vol. 1 §1.5.3).
pub(crate) fn galactic_to_equatorial(l: f32, b: f32) -> (f32, f32) {
    const POLE_RA: f32 = 192.859_48 * PI / 180.0;
    const POLE_DEC: f32 = 27.128_25 * PI / 180.0;
    const NCP_L: f32 = 122.931_92 * PI / 180.0;
    let (sin_dec_pole, cos_dec_pole) = POLE_DEC.sin_cos();
    let (sin_b, cos_b) = b.sin_cos();
    let (sin_dl, cos_dl) = (NCP_L - l).sin_cos();
    let sin_dec = (sin_dec_pole * sin_b + cos_dec_pole * cos_b * cos_dl).clamp(-1.0, 1.0);
    let y = cos_b * sin_dl;
    let x = cos_dec_pole * sin_b - sin_dec_pole * cos_b * cos_dl;
    let ra = (POLE_RA + y.atan2(x)).rem_euclid(TAU);
    (ra, sin_dec.asin())
}

/// The magnitude at quantile `u` of the star-count law `N(<m) ∝ 10^(0.6 m)` over [`MAG_RANGE`],
/// its inverse in closed form: faint stars outnumber bright ones by about 4 to 1 a magnitude.
pub(crate) fn magnitude(u: f32) -> f32 {
    let (lo, hi) = MAG_RANGE;
    let (a, b) = (10_f32.powf(0.6 * lo), 10_f32.powf(0.6 * hi));
    (a + u * (b - a)).log10() / 0.6
}

/// The unit vector of `(ra, dec)`.
pub(crate) fn unit_vector(ra: f32, dec: f32) -> Vec3 {
    let (sin_dec, cos_dec) = dec.sin_cos();
    let (sin_ra, cos_ra) = ra.sin_cos();
    Vec3::new(cos_dec * cos_ra, cos_dec * sin_ra, sin_dec)
}

/// Each star brighter than [`ASTERISM_MAG`] joined to its nearest such star within
/// [`ASTERISM_REACH`], each pair once.
fn asterisms(objects: &[Object]) -> Vec<[ObjectId; 2]> {
    let bright: Vec<(ObjectId, Vec3)> = objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.kind == ObjectKind::Star && object.mag < ASTERISM_MAG)
        .map(|(idx, object)| (ObjectId(idx as u32), unit_vector(object.ra, object.dec)))
        .collect();
    let reach = ASTERISM_REACH.cos();
    let mut lines = Vec::new();
    for &(id, at) in &bright {
        let nearest = bright
            .iter()
            .filter(|(other, _)| *other != id)
            .map(|&(other, there)| (other, at.dot(there)))
            .filter(|&(_, cos)| cos >= reach)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((other, _)) = nearest {
            let pair = if id < other { [id, other] } else { [other, id] };
            if !lines.contains(&pair) {
                lines.push(pair);
            }
        }
    }
    lines
}

/// SplitMix64 (Steele, Lea and Flood, "Fast splittable pseudorandom number generators", OOPSLA
/// 2014): a generator whose output this file fixes, where a crate's could move with a version.
#[derive(Debug)]
pub(crate) struct SplitMix64(pub(crate) u64);

impl SplitMix64 {
    pub(crate) const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..1`, from the top 24 bits, the precision an `f32` holds.
    pub(crate) fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Standard normal, by the Box–Muller transform of two uniforms.
    fn normal(&mut self) -> f32 {
        let u = 1.0 - self.unit();
        let v = self.unit();
        (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use crate::app::model::catalog::{
        Catalog, DEEP_SKY_COUNT, ObjectId, ObjectKind, SEED, STAR_COUNT, SplitMix64,
        galactic_to_equatorial, magnitude,
    };

    /// SplitMix64's published first outputs for seed 0
    /// (<https://prng.di.unimi.it/splitmix64.c>, checked against its reference run).
    #[test]
    fn splitmix_matches_the_reference_sequence() {
        let mut rng = SplitMix64(0);
        assert_eq!(rng.next(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(rng.next(), 0x06c4_5d18_8009_454f);
    }

    /// The law's ends: quantile 0 is the brightest magnitude, 1 the faintest; and the median sits
    /// where `10^(0.6 m)` is halfway: `log10((10^-0.6 + 10^3.9) / 2) / 0.6`.
    #[test]
    fn the_magnitude_law_spans_its_range() {
        assert!((magnitude(0.0) - -1.0).abs() < 1e-5);
        assert!((magnitude(1.0) - 6.5).abs() < 1e-5);
        let median = f32::midpoint(10_f32.powf(-0.6), 10_f32.powf(3.9)).log10() / 0.6;
        assert!((magnitude(0.5) - median).abs() < 1e-5, "{}", magnitude(0.5));
        assert!((median - 5.998).abs() < 1e-3, "{median}");
    }

    /// The galactic pole and center land where the IAU frame puts them: b = 90° at RA 192.859°,
    /// Dec 27.128°; l = 0, b = 0 at RA 266.405°, Dec −28.936°.
    #[test]
    fn the_galactic_frame_lands_on_its_reference_points() {
        let deg = PI / 180.0;
        let (ra, dec) = galactic_to_equatorial(0.0, FRAC_PI_2);
        assert!((ra / deg - 192.859).abs() < 0.01, "{}", ra / deg);
        assert!((dec / deg - 27.128).abs() < 0.01, "{}", dec / deg);
        let (ra, dec) = galactic_to_equatorial(0.0, 0.0);
        assert!((ra / deg - 266.405).abs() < 0.01, "{}", ra / deg);
        assert!((dec / deg - -28.936).abs() < 0.01, "{}", dec / deg);
    }

    /// One seed makes one sky: its counts, its first star's designation, and the same objects on
    /// a second run; every coordinate in range; asterisms only between bright stars.
    #[test]
    fn a_seed_makes_one_sky() {
        let sky = Catalog::generate(SEED);
        assert_eq!(sky.len(), (STAR_COUNT + DEEP_SKY_COUNT) as usize);
        assert_eq!(sky.get(ObjectId(0)).name, "OBS 0001");
        assert_eq!(sky.get(ObjectId(STAR_COUNT)).name, "DSO 01");
        assert_eq!(sky.get(ObjectId(STAR_COUNT)).kind, ObjectKind::Cluster);
        let again = Catalog::generate(SEED);
        assert!(sky.iter().zip(again.iter()).all(|(a, b)| a == b));
        assert_eq!(sky.lines(), again.lines());
        for (_, object) in sky.iter() {
            assert!((0.0..std::f32::consts::TAU).contains(&object.ra));
            assert!((-FRAC_PI_2..=FRAC_PI_2).contains(&object.dec));
        }
        assert!(!sky.lines().is_empty());
        for [a, b] in sky.lines() {
            assert!(a < b);
            assert!(sky.get(*a).mag < 2.6 && sky.get(*b).mag < 2.6);
        }
        let other = Catalog::generate(SEED + 1);
        assert_ne!(sky.get(ObjectId(0)).ra, other.get(ObjectId(0)).ra);
    }
}
