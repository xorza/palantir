use crate::common::hash::*;

#[test]
fn pod_slice_matches_write_of_bytes_and_chunks_by_word() {
    #[repr(C)]
    #[derive(Debug, Clone, Copy, bytemuck::NoUninit)]
    struct Pair {
        a: u32,
        b: u32,
    }
    let pairs = [
        Pair {
            a: 0x1234_5678,
            b: 0x9abc_def0,
        },
        Pair { a: 1, b: 2 },
    ];
    let mut bulk = Hasher::new();
    bulk.pod_slice(&pairs);
    let mut bytes = Hasher::new();
    bytes.write(bytemuck::cast_slice(&pairs));
    assert_eq!(bulk.finish(), bytes.finish(), "case: &[Pair]");

    // `FxHasher::write` consumes `usize` chunks, so one 16-byte write differs from two 8-byte writes: bulk and per-element hashing are not interchangeable.
    let mut per_element = Hasher::new();
    for p in &pairs {
        per_element.write(bytemuck::bytes_of(p));
    }
    assert_ne!(
        per_element.finish(),
        bulk.finish(),
        "if these ever coincide the chunking contract changed — \
         re-read pod_slice's docs before relying on either form",
    );
}

#[test]
fn pod_slice_length_is_not_folded_in() {
    // `pod_slice` hashes bytes only, so splits of one byte run collide; callers hashing a variable-length column write the length.
    let a: [u32; 2] = [0x1111_1111, 0x2222_2222];
    let b: [u16; 4] = [0x1111, 0x1111, 0x2222, 0x2222];
    let mut ha = Hasher::new();
    ha.pod_slice(&a);
    let mut hb = Hasher::new();
    hb.pod_slice(&b);
    assert_eq!(
        ha.finish(),
        hb.finish(),
        "same bytes must hash the same regardless of element split",
    );
}

#[test]
fn new_matches_default_seed() {
    // `Hasher::new` wraps `FxHasher::default`; a custom seed would silently change every cache key.
    let mut wrapped = Hasher::new();
    let mut raw = FxHasher::default();
    let bytes: &[u8] = b"palantir";
    wrapped.write(bytes);
    raw.write(bytes);
    assert_eq!(wrapped.finish(), raw.finish());
}
