# Open issues

- **A material's fill and border overcount the outer edge.** `palantir_composite` and
  `palantir_composite_outside` (`src/effect/material.wgsl`) give the fill `outer_aa` and composite
  the border over it, so a pixel half inside an opaque material box with an opaque border reads
  alpha 0.75 where its coverage is 0.5.
