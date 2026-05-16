//! Cross-module test/bench fixtures. Most helpers live in `test_support`
//! mods inside each production file (canonical paths
//! `crate::foo::bar::test_support::*`); this module is reserved for
//! fixtures that genuinely span modules and would feel arbitrary
//! living in any one of them.

#[cfg(any(test, feature = "internals"))]
pub mod testing;
