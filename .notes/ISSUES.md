# Open issues

- `cargo doc -p palantir --no-deps` fails: the `Deserialize for AnimSpec` doc in
  `src/animation/anim_spec/mod.rs:146` links `DURATION_ERROR` and `SPRING_ERROR`,
  which are private, and the manifest denies `rustdoc::private_intra_doc_links`.
