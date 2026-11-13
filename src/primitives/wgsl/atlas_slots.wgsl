// Whether a read of atlas page `page`, among the open page slots `slots` (slot `i` at bit `i`),
// takes slot `slot`: an open slot, and the page's own or the last open one, which needs no test.
// Called with `slot` 0, 1 and 2 in turn before a read of slot 3, under a `slots` the pipeline pins,
// the compiler drops every branch it can decide: over one open page the read loads it and never
// branches, wherever the page sits (`PageSlots`).
fn palantir_reads_slot(slots: u32, slot: u32, page: u32) -> bool {
    return (slots & (1u << slot)) != 0u && (page == slot || (slots >> (slot + 1u)) == 0u);
}
