//! The `dcrs-port` command line: a thin wrapper over the `dcrs_port` library.
//!
//! Everything substantive lives in the library so it can be tested and driven from a pipeline; this
//! file only handles argument parsing, file IO, and exit codes.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use dcrs_compat::Registry;
use dcrs_port::{analyze, convert, effects, theme};

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
    /// Convert a theme to a native Serein `.serein-extension` package.
    Convert {
        /// Path to a `.css` or `.theme.css` file.
        path: PathBuf,

        /// Where to write the package. Defaults to `<theme>.serein-extension` beside the input.
        #[arg(long, short)]
        out: Option<PathBuf>,

        /// Embed a background image, as a PNG or JPEG on disk.
        ///
        /// A Serein theme cannot reference an image URL, so a theme with a background has to carry
        /// the bytes. Any `data:` image already in the CSS is used automatically.
        #[arg(long, value_name = "PNG_OR_JPEG")]
        background: Option<PathBuf>,

        /// Theme name. Defaults to the `@name` header, then the file stem.
        #[arg(long)]
        name: Option<String>,

        /// Theme author. Defaults to the `@author` header.
        #[arg(long)]
        author: Option<String>,

        /// Theme licence. Defaults to the `@license` header.
        #[arg(long)]
        license: Option<String>,

        /// Theme version. Defaults to the `@version` header.
        #[arg(long)]
        version: Option<String>,

        /// Source repository URL. Defaults to the `@source` header. Must be HTTPS or empty.
        #[arg(long)]
        source: Option<String>,
    },
    /// Summarize the capability registry.
    Coverage,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let registry = load_registry(cli.registry.as_deref())?;
    let class_map = load_class_map(cli.class_map.as_deref())?;
    dispatch(&cli, &registry, &class_map)
}

/// Runs one subcommand.
fn dispatch(
    cli: &Cli,
    registry: &Registry,
    class_map: &dcrs_theme::ClassMap,
) -> anyhow::Result<()> {
    match &cli.command {
        Command::Plugin { path } => {
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let report = analyze::analyze(&source, registry)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
        }
        Command::Theme { path } => {
            let source = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let report = theme::analyze(&source, class_map);
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
        Command::Convert {
            path,
            out,
            background,
            name,
            author,
            license,
            version,
            source,
        } => {
            let overrides = convert::Overrides {
                name: name.clone(),
                author: author.clone(),
                license: license.clone(),
                version: version.clone(),
                source: source.clone(),
            };
            convert_command(cli, path, out.as_deref(), background.as_deref(), overrides)?;
        }
        Command::Coverage => {
            if cli.json {
                let json = theme::coverage_json(registry);
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                print!("{}", theme::render_coverage(registry));
            }
        }
    }
    Ok(())
}

/// Runs `dcrs-port convert`.
///
/// Exits non-zero when no package can be produced: the report is still printed, because it is what
/// explains the failure, but a pipeline should not treat this as a successful conversion.
fn convert_command(
    cli: &Cli,
    path: &Path,
    out: Option<&Path>,
    background: Option<&Path>,
    overrides: convert::Overrides,
) -> anyhow::Result<()> {
    let source_text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let stem = path
        .file_stem()
        .map_or_else(|| "theme".to_owned(), |s| s.to_string_lossy().into_owned());

    // One parse for the conversion and the image search together; a second parse would hold two copies
    // of every rule at once.
    let (mut report, embedded) = convert::convert_file(&source_text, &stem)
        .with_context(|| format!("parsing {}", path.display()))?;
    report.apply(overrides);

    let image = match background {
        Some(image_path) => Some(read_image(image_path)?),
        None => embedded,
    };

    match report.package(image.unwrap_or_default()) {
        Ok(built) => {
            let destination =
                out.map_or_else(|| default_out(path, &report.name), Path::to_path_buf);
            std::fs::write(&destination, &built.json)
                .with_context(|| format!("writing {}", destination.display()))?;
            if cli.json {
                let json_report = convert_json(&report, &destination);
                println!("{}", serde_json::to_string_pretty(&json_report)?);
            } else {
                print!("{}", report.render());
                println!("wrote {}", destination.display());
            }
        }
        Err(e) => {
            report.failure = Some(e.to_string());
            if cli.json {
                let json_report = convert_json(&report, Path::new(""));
                println!("{}", serde_json::to_string_pretty(&json_report)?);
            } else {
                print!("{}", report.render());
            }
            std::process::exit(2);
        }
    }
    Ok(())
}

/// Where a converted package goes when `--out` is not given.
fn default_out(input: &Path, name: &str) -> PathBuf {
    let stem = dcrs_serein::safe_id(name);
    input.with_file_name(format!("{stem}.serein-extension"))
}

/// Reads an image, refusing anything past the host's byte budget before allocating for it.
fn read_image(path: &Path) -> anyhow::Result<Vec<u8>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() <= dcrs_serein::MAX_BACKGROUND_BYTES,
        "{} is {} bytes; Serein allows at most {}",
        path.display(),
        bytes.len(),
        dcrs_serein::MAX_BACKGROUND_BYTES
    );
    Ok(bytes)
}

/// The JSON form of a conversion report.
fn convert_json(report: &convert::ConversionReport, out: &Path) -> serde_json::Value {
    serde_json::json!({
        "name": report.name,
        "author": report.author,
        "license": report.license,
        "version": report.version,
        "source": report.source,
        "out": out.display().to_string(),
        "tokens_mapped": report.tokens_mapped(),
        "coverage": report.conversion.coverage(),
        "variables_seen": report.conversion.variables_seen,
        "lossless": report.is_lossless(),
        "failure": report.failure,
        "dropped": report.conversion.dropped.iter().map(|d| serde_json::json!({
                    "target": d.target,
                    "variable": d.variable,
                    "reason": d.reason,
                })).collect::<Vec<_>>(),
        "host_limits": convert::host_limits()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<std::collections::BTreeMap<_, _>>(),
    })
}

fn load_registry(path: Option<&Path>) -> anyhow::Result<Registry> {
    match path {
        Some(p) => Registry::load(p).with_context(|| format!("loading registry {}", p.display())),
        None => Ok(Registry::default()),
    }
}

fn load_class_map(path: Option<&Path>) -> anyhow::Result<dcrs_theme::ClassMap> {
    match path {
        Some(p) => dcrs_theme::ClassMap::load(p)
            .with_context(|| format!("loading class map {}", p.display())),
        None => Ok(dcrs_theme::ClassMap::default()),
    }
}
