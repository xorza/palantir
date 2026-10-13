//! [`Surface`]: the union of every build's exports, rendered as the surface document.

use std::collections::{BTreeMap, BTreeSet};

use crate::build::Build;
use crate::renderer::BuildSurface;

/// Every export path over every build, with the builds that export it.
#[derive(Debug)]
pub(crate) struct Surface {
    builds: Vec<(Build, BuildSurface)>,
}

impl Surface {
    pub(crate) const fn new(builds: Vec<(Build, BuildSurface)>) -> Self {
        Self { builds }
    }

    fn paths(&self) -> BTreeSet<&str> {
        self.builds
            .iter()
            .flat_map(|(_, surface)| surface.blocks.keys().map(String::as_str))
            .collect()
    }

    pub(crate) fn item_count(&self) -> usize {
        self.paths().len()
    }

    pub(crate) fn unreached(&self) -> BTreeSet<&str> {
        self.builds
            .iter()
            .flat_map(|(_, surface)| surface.unreached.iter().map(String::as_str))
            .collect()
    }

    /// The tag after a path that not every build exports alike: the features any of which
    /// brings it, or every build that drops it, and whether its declaration differs between
    /// them.
    fn tag(&self, path: &str) -> String {
        let holders: Vec<&(Build, BuildSurface)> = self
            .builds
            .iter()
            .filter(|(_, surface)| surface.blocks.contains_key(path))
            .collect();
        let names = |builds: &mut dyn Iterator<Item = &Build>, separator: &str| {
            builds
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(separator)
        };
        let in_bare = holders.iter().any(|(build, _)| *build == Build::NoDefault);
        let mut tag = if !in_bare {
            format!(
                "  [features: {}]",
                names(&mut holders.iter().map(|(build, _)| build), " or ")
            )
        } else if holders.len() < self.builds.len() {
            let dropping = self
                .builds
                .iter()
                .filter(|(_, surface)| !surface.blocks.contains_key(path))
                .map(|(build, _)| build);
            format!("  [dropped by: {}]", names(&mut { dropping }, ", "))
        } else {
            String::new()
        };
        let shapes: BTreeSet<&Vec<String>> = holders
            .iter()
            .map(|(_, surface)| &surface.blocks[path])
            .collect();
        if shapes.len() > 1 {
            tag += "  [shape differs by feature]";
        }
        tag
    }

    /// The surface document: the exports, the prelude, and the public items no path reaches.
    pub(crate) fn markdown(&self) -> String {
        let prelude: BTreeSet<&str> = self
            .builds
            .iter()
            .flat_map(|(_, surface)| surface.prelude.iter().map(String::as_str))
            .collect();
        let prelude: Vec<String> = prelude.iter().map(|name| format!("`{name}`")).collect();
        // The first build to export a path gives its block: the bare build, where it has one.
        let blocks: BTreeMap<&str, &Vec<String>> = self
            .builds
            .iter()
            .rev()
            .flat_map(|(_, surface)| surface.blocks.iter().map(|(p, b)| (p.as_str(), b)))
            .collect();
        let mut lines: Vec<String> = [
            "# Public API surface",
            "",
            "Every item the crate exports, rendered as its declaration from rustdoc JSON",
            "(`cargo run -p api-surface`), sorted by path. The surface is the union of a build with",
            "no features and one build per public feature; a tag names the builds an item needs or",
            "loses. `internals` is the crate's own test surface and is left out.",
            "",
        ]
        .map(String::from)
        .into();
        lines.push(format!(
            "`prelude` re-exports these root items: {}.",
            prelude.join(", ")
        ));
        lines.extend(["", "```text"].map(String::from));
        for (path, block) in blocks {
            lines.push(format!("{path}{}", self.tag(path)));
            lines.extend(block.iter().map(|line| format!("    {line}")));
            lines.push(String::new());
        }
        lines.extend(
            [
                "```",
                "",
                "## Public but not exported",
                "",
                "Items declared `pub` that no path from the crate root reaches. A caller cannot name",
                "them; each one is either dead surface or a type that leaks through a signature.",
                "",
                "```text",
            ]
            .map(String::from),
        );
        let unreached = self.unreached();
        if unreached.is_empty() {
            lines.push("(none)".to_owned());
        }
        lines.extend(unreached.into_iter().map(String::from));
        lines.push("```".to_owned());
        lines.push(String::new());
        lines.join("\n")
    }
}
