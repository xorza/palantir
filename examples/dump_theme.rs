// Serialize the default `Theme` to RON and write it next to the
// example. Run with `cargo run --example dump_theme` — produces
// `examples/theme.ron` and prints the same content to stdout.

use palantir::Theme;
use ron::ser::PrettyConfig;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let theme = Theme::default();
    let encoded = ron::ser::to_string_pretty(&theme, PrettyConfig::default())?;

    let out: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("theme.ron");
    fs::write(&out, &encoded)?;

    println!("// wrote {}\n{encoded}", out.display());

    let parsed: Theme = ron::from_str(&encoded)?;
    let reroundtripped = ron::ser::to_string_pretty(&parsed, PrettyConfig::default())?;
    assert_eq!(encoded, reroundtripped, "RON round-trip diverged");
    Ok(())
}
