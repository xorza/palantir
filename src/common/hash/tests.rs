use crate::common::hash::*;

#[test]
fn pod_slice_matches_write_of_bytes_and_chunks_by_word() {
    // `pod_slice` is only safe as a shortcut if it feeds exactly the
    // slice's bytes through `write`.
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

    // `FxHasher::write` consumes `usize`-sized chunks, so one 16-byte
    // write does not land in the same state as two 8-byte writes. Bulk
    // and per-element hashing are therefore *not* interchangeable,
    // however natural the swap looks at a call site. Pinned in the
    // surprising direction on purpose: a caller who assumes equivalence
    // for a persisted key gets a silent mismatch rather than a failure.
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
    // Documented contract: `pod_slice` hashes bytes only. Two
    // different splits of the same byte run collide, which is why
    // callers hashing a variable-length column must write the
    // length themselves. Pinned so the omission stays a deliberate
    // property rather than a latent surprise.
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
    // `Hasher::new` is a thin wrapper over `FxHasher::default`. If
    // a future refactor adds a custom seed without updating call
    // sites, every cache key changes silently — pin the equality.
    let mut wrapped = Hasher::new();
    let mut raw = FxHasher::default();
    let bytes: &[u8] = b"palantir";
    wrapped.write(bytes);
    raw.write(bytes);
    assert_eq!(wrapped.finish(), raw.finish());
}
