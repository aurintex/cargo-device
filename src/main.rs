//! `cargo-device` — cross-compile, deploy, and run Rust binaries on embedded Linux devices.
//!
//! Invoked by cargo as `cargo device <subcommand>`.

mod build;
mod config;
mod deploy;
mod device;
mod error;
mod run;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cargo-device",
    bin_name = "cargo device",
    version,
    about = "Cross-compile, deploy, and run Rust binaries on embedded Linux devices"
)]
struct Cli {
    /// Enable debug-level log output
    #[arg(short = 'v', long)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build for a device
    Build {
        /// Device name as defined in .cargo/config.toml
        device: String,
        /// Extra arguments passed through to `cargo build`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cargo_args: Vec<String>,
    },
    /// Build, deploy, and run on a device (stdout/stderr streamed locally)
    Run {
        /// Device name as defined in .cargo/config.toml
        device: String,
        /// Extra arguments passed through to `cargo build`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cargo_args: Vec<String>,
    },
    /// Sync directories to a device without building or running
    Sync {
        /// Device name as defined in .cargo/config.toml
        device: String,
    },
    /// List all configured devices
    List,
}

fn main() -> Result<()> {
    // Parse CLI before initialising tracing so verbosity is known upfront.
    // When invoked via `cargo device ...`, cargo inserts "device" as the first argument — skip it.
    let args: Vec<_> = std::env::args_os()
        .enumerate()
        .filter(|(i, arg)| !(*i == 1 && arg == "device"))
        .map(|(_, arg)| arg)
        .collect();
    let cli = Cli::parse_from(args);

    let level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()),
        )
        .init();

    let cfg = config::load()?;

    match cli.command {
        Command::List => list_devices(&cfg),
        Command::Build { device, cargo_args } => {
            let dev = device::resolve(&cfg, &device)?;
            build::run(&dev, &cargo_args)?;
        }
        Command::Run { device, cargo_args } => {
            let dev = device::resolve(&cfg, &device)?;
            let is_release = cargo_args.iter().any(|a| a == "--release");
            let bin_name = read_package_name()?;
            build::run(&dev, &cargo_args)?;
            deploy::run(&dev, is_release, &bin_name)?;
            run::execute(&dev, &bin_name, &cargo_args)?;
        }
        Command::Sync { device } => {
            let dev = device::resolve(&cfg, &device)?;
            deploy::sync_dirs(&dev)?;
        }
    }

    Ok(())
}

fn list_devices(cfg: &config::Config) {
    if cfg.device.is_empty() {
        println!("No devices configured. Add [device.*] tables to .cargo/config.toml.");
        return;
    }
    let name_w = cfg.device.keys().map(|k| k.len()).max().unwrap_or(4).max(4);
    let target_w = cfg
        .device
        .values()
        .map(|d| d.target.as_deref().unwrap_or("\u{2014}").len())
        .max()
        .unwrap_or(6)
        .max(6);
    println!("{:<name_w$}  {:<target_w$}  HOST", "NAME", "TARGET");
    let mut names: Vec<_> = cfg.device.keys().collect();
    names.sort();
    for name in names {
        let d = &cfg.device[name];
        let target = d.target.as_deref().unwrap_or("\u{2014}");
        let host = d.ssh_host.as_deref().unwrap_or("local");
        println!("{name:<name_w$}  {target:<target_w$}  {host}");
    }
}

fn read_package_name() -> Result<String> {
    #[derive(serde::Deserialize)]
    struct CargoToml {
        package: Package,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        name: String,
    }
    let content = std::fs::read_to_string("Cargo.toml")
        .context("failed to read Cargo.toml — run cargo device from the project root")?;
    let parsed: CargoToml =
        toml::from_str(&content).context("failed to parse Cargo.toml [package]")?;
    Ok(parsed.package.name)
}
