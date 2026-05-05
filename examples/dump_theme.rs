// Serialize the default `Theme` to TOML and write it next to the
// example. Run with `cargo run --example dump_theme` — produces
// `examples/theme.toml` and prints the same content to stdout.

use palantir::Theme;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let theme = Theme::default();
    let toml = toml::to_string_pretty(&theme)?;

    let out: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("theme.toml");
    fs::write(&out, &toml)?;

    println!("// wrote {}\n{toml}", out.display());

    let parsed: Theme = toml::from_str(&toml)?;
    let reroundtripped = toml::to_string_pretty(&parsed)?;
    assert_eq!(toml, reroundtripped, "TOML round-trip diverged");
    Ok(())
}
