//! `dcrs-port` — analyze Discord themes and mods, report how portable they are to a native client.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]
// Report rendering is far clearer as a sequence of `push_str(&format!(..))` calls than as one
// deeply nested `write!` chain.
#![allow(clippy::format_push_string)]

mod analyze;
mod effects;
mod theme;

use std::path::PathBuf;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use dcrs_compat::Registry;

#[derive(Debug, Parser)]
#[command(name = "dcrs-port", about, version)]
struct Cli {
    /// Path to the capability registry TOML. Falls back to the bundled default.
    #[arg(long, global = true)]
    registry: Option<PathBuf>,

    /// Path to the class map JSON. Without it, hashed class names are reported untranslated.
    #[arg(long, global = true)]
    class_map: Option<PathBuf>,

    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyze a plugin source file.
    Plugin {
        /// Path to a `.ts`, `.tsx`, or `.js` plugin source.
        path: PathBuf,
    },
    /// Analyze a theme CSS file.
    Theme {
        /// Path to a `.css` or `.theme.css` file.
        path: PathBuf,
    },
    /// Report what a plugin actually does, independent of how it does it.
    Effects {
        /// Path to a plugin source file.
        path: PathBuf,
    },
    /// Summarize the capability registry.
    Coverage,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let registry = load_registry(cli.registry.as_deref())?;
    let class_map = load_class_map(cli.class_map.as_deref())?;

    match &cli.command {
        Command::Plugin { path } => {
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let report = analyze::analyze(&source, &registry)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
        }
        Command::Theme { path } => {
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let report = theme::analyze(&source, &class_map);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
        }
        Command::Effects { path } => {
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let report = effects::Effects::extract(&source);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("plugin effects\n{}", report.render());
            }
            // A plugin with effects the client cannot reproduce is worth a non-zero exit, so this
            // can gate a porting pipeline rather than just inform one.
            if report.needs_manual_work() {
                std::process::exit(2);
            }
        }
        Command::Coverage => {
            if cli.json {
                let json = theme::coverage_json(&registry);
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                print!("{}", theme::render_coverage(&registry));
            }
        }
    }
    Ok(())
}

fn load_registry(path: Option<&std::path::Path>) -> anyhow::Result<Registry> {
    match path {
        Some(p) => Registry::load(p).with_context(|| format!("loading registry {}", p.display())),
        None => Ok(Registry::default()),
    }
}

fn load_class_map(path: Option<&std::path::Path>) -> anyhow::Result<dcrs_theme::ClassMap> {
    match path {
        Some(p) => dcrs_theme::ClassMap::load(p)
            .with_context(|| format!("loading class map {}", p.display())),
        None => Ok(dcrs_theme::ClassMap::default()),
    }
}
