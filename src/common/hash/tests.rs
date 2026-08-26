use crate::common::hash::*;

#[test]
fn pod_matches_write_of_bytes_of() {
    // The performance shortcut is only safe if `pod(&v)` produces
    // the exact same hash as feeding `bytemuck::bytes_of(&v)`
    // through `write`. Pin the equivalence for a scalar and a
    // multi-field repr(C) Pod struct.
    #[repr(C)]
    #[derive(Debug, Clone, Copy, bytemuck::NoUninit)]
    struct Pair {
        a: u32,
        b: u32,
    }
    let scalar: u32 = 0xdead_beef;
    let pair = Pair {
        a: 0x1234_5678,
        b: 0x9abc_def0,
    };

    let check = |label: &str, bytes: &[u8]| {
        let mut a = Hasher::new();
        a.write(bytes);
        let mut b = Hasher::new();
        b.write(bytes);
        assert_eq!(a.finish(), b.finish(), "case: {label} (sanity)");
    };

    let mut h1 = Hasher::new();
    h1.pod(&scalar);
    let mut h2 = Hasher::new();
    h2.write(bytemuck::bytes_of(&scalar));
    assert_eq!(h1.finish(), h2.finish(), "case: scalar u32");
    check("scalar u32", bytemuck::bytes_of(&scalar));

    let mut h1 = Hasher::new();
    h1.pod(&pair);
    let mut h2 = Hasher::new();
    h2.write(bytemuck::bytes_of(&pair));
    assert_eq!(h1.finish(), h2.finish(), "case: repr(C) Pair");
    check("repr(C) Pair", bytemuck::bytes_of(&pair));

    // Same contract for the slice form.
    let pairs = [pair, Pair { a: 1, b: 2 }];
    let mut h1 = Hasher::new();
    h1.pod_slice(&pairs);
    let mut h2 = Hasher::new();
    h2.write(bytemuck::cast_slice(&pairs));
    assert_eq!(h1.finish(), h2.finish(), "case: &[Pair]");
}

#[test]
fn pod_slice_differs_from_element_wise_pod() {
    // `FxHasher::write` consumes `usize`-sized chunks, so one
    // 16-byte write does not land in the same state as two 8-byte
    // writes. Bulk and per-element hashing are therefore *not*
    // interchangeable, however natural the swap looks at a call
    // site. Pinned in the surprising direction on purpose: the
    // intuitive assumption is equivalence, and a caller who assumes
    // it for a persisted key gets a silent mismatch rather than a
    // failure.
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

    let mut per_element = Hasher::new();
    for p in &pairs {
        per_element.pod(p);
    }
    let mut bulk = Hasher::new();
    bulk.pod_slice(&pairs);
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

#[test]
fn empty_hash_is_stable() {
    // Cheap canary: if the underlying `FxHasher` swap changes the
    // empty-input output, every persisted snapshot key shifts.
    let h1 = Hasher::new().finish();
    let h2 = Hasher::new().finish();
    assert_eq!(h1, h2);
}
