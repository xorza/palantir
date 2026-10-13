//! Writes `.notes/API_SURFACE.md`: every item palantir exports, rendered as its declaration from
//! rustdoc JSON, over a build with no features and one build per public feature.
//!
//! `cargo run -p api-surface` needs a nightly toolchain installed, as rustdoc JSON is
//! nightly-only. CI runs it and fails on any difference from the committed file.

mod build;
mod decl;
mod error;
mod renderer;
mod surface;
mod workspace;

use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use crate::error::Error;
use crate::renderer::Renderer;
use crate::surface::Surface;
use crate::workspace::Workspace;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut causes = vec![error.to_string()];
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                causes.push(cause.to_string());
                source = cause.source();
            }
            writeln!(io::stderr(), "api-surface: {}", causes.join(": ")).ok();
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("api-surface sits in palantir's workspace root");
    let workspace = Workspace::load(root)?;
    let mut builds = Vec::new();
    for build in workspace.builds() {
        writeln!(io::stderr(), "rustdoc JSON: {build}").ok();
        let krate = workspace.rustdoc(&build)?;
        builds.push((build, Renderer::render(&krate)));
    }
    let surface = Surface::new(builds);
    let path = workspace.root().join(".notes").join("API_SURFACE.md");
    fs::write(&path, surface.markdown()).map_err(|source| Error::Write {
        path: path.clone(),
        source,
    })?;
    writeln!(
        io::stderr(),
        "wrote {}: {} items, {} unreached",
        path.display(),
        surface.item_count(),
        surface.unreached().len()
    )
    .ok();
    Ok(())
}
