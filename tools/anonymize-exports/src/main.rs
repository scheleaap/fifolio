//! The command line around [`anonymize_exports::run`].

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// Where the real exports are, and where the fixtures go.
#[derive(Debug, Parser)]
#[command(about = "Derive the committed test fixtures from the real broker exports")]
struct Arguments {
    /// The directory holding `saxo-nl/` and `trade-republic/`.
    #[arg(long, default_value = "design/example_exports")]
    source: PathBuf,
    /// The directory the fixtures are written to.
    #[arg(long, default_value = "fixtures")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    anonymize_exports::run(&arguments.source, &arguments.out)
}
