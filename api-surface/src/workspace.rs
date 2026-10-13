//! [`Workspace`]: palantir's manifest facts, and the rustdoc JSON it builds.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustdoc_types::{Crate, FORMAT_VERSION};
use serde_json::Value;

use crate::build::Build;
use crate::error::Error;

/// Features that are not public surface: the default set, and the crate's own test surface.
const NOT_SURFACE: [&str; 2] = ["default", "internals"];

/// Where palantir lives, where cargo writes its docs, and the features a caller can turn on.
#[derive(Debug)]
pub(crate) struct Workspace {
    root: PathBuf,
    rustdoc_json: PathBuf,
    features: Vec<String>,
}

impl Workspace {
    /// palantir's workspace at `root`, as `cargo metadata` reads it.
    pub(crate) fn load(root: &Path) -> Result<Self, Error> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(root)
            .stderr(Stdio::inherit())
            .output()
            .map_err(Error::Spawn)?;
        if !output.status.success() {
            return Err(Error::Cargo {
                command: "cargo metadata",
                status: output.status,
            });
        }
        let metadata: Value = serde_json::from_slice(&output.stdout)?;
        let target = metadata["target_directory"]
            .as_str()
            .ok_or(Error::Metadata("target directory"))?;
        let package = metadata["packages"]
            .as_array()
            .and_then(|packages| packages.iter().find(|p| p["name"] == "palantir"))
            .ok_or(Error::Metadata("palantir package"))?;
        let mut features: Vec<String> = package["features"]
            .as_object()
            .ok_or(Error::Metadata("palantir features"))?
            .keys()
            .filter(|feature| !NOT_SURFACE.contains(&feature.as_str()))
            .cloned()
            .collect();
        features.sort_unstable();
        Ok(Self {
            root: root.to_owned(),
            rustdoc_json: Path::new(target).join("doc").join("palantir.json"),
            features,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// A build with no features, then one per public feature.
    pub(crate) fn builds(&self) -> impl Iterator<Item = Build> + '_ {
        std::iter::once(Build::NoDefault).chain(self.features.iter().cloned().map(Build::Feature))
    }

    /// palantir's rustdoc JSON under `build`, from the nightly toolchain, which alone writes it.
    pub(crate) fn rustdoc(&self, build: &Build) -> Result<Crate, Error> {
        // Every build writes the one file, so the last build's is removed first: a build cargo
        // skipped then fails to read, rather than reading another feature set's surface.
        match fs::remove_file(&self.rustdoc_json) {
            Err(source) if source.kind() != io::ErrorKind::NotFound => {
                return Err(Error::Remove {
                    path: self.rustdoc_json.clone(),
                    source,
                });
            }
            Ok(()) | Err(_) => {}
        }
        let mut command = Command::new("cargo");
        command
            .args([
                "+nightly",
                "rustdoc",
                "-p",
                "palantir",
                "--lib",
                "--no-default-features",
            ])
            .current_dir(&self.root)
            .stdout(Stdio::null());
        if let Build::Feature(feature) = build {
            command.args(["--features", feature]);
        }
        command.args([
            "--",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--document-hidden-items",
        ]);
        let status = command.status().map_err(Error::Spawn)?;
        if !status.success() {
            return Err(Error::Cargo {
                command: "cargo +nightly rustdoc",
                status,
            });
        }
        let bytes = fs::read(&self.rustdoc_json).map_err(|source| Error::Read {
            path: self.rustdoc_json.clone(),
            source,
        })?;
        // The version first, so another format fails on it rather than on a field it renamed.
        let json: Value = serde_json::from_slice(&bytes)?;
        let found = json["format_version"].as_u64();
        if found != Some(u64::from(FORMAT_VERSION)) {
            return Err(Error::FormatVersion { found });
        }
        Ok(serde_json::from_value(json)?)
    }
}
