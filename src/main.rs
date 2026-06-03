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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // When invoked via `cargo device ...`, cargo inserts "device" as the first argument — skip it.
    let args: Vec<_> = std::env::args_os()
        .enumerate()
        .filter(|(i, arg)| !(*i == 1 && arg == "device"))
        .map(|(_, arg)| arg)
        .collect();

    let cli = Cli::parse_from(args);

    let device_name = match &cli.command {
        Command::Build { device, .. } | Command::Run { device, .. } | Command::Sync { device } => {
            device.clone()
        }
    };
    let cfg = config::load()?;
    let dev = device::resolve(&cfg, &device_name)?;

    match cli.command {
        Command::Build { cargo_args, .. } => {
            build::run(&dev, &cargo_args)?;
        }
        Command::Run { cargo_args, .. } => {
            let is_release = cargo_args.iter().any(|a| a == "--release");
            let bin_name = read_package_name()?;
            build::run(&dev, &cargo_args)?;
            deploy::run(&dev, is_release, &bin_name)?;
            run::execute(&dev, &bin_name)?;
        }
        Command::Sync { .. } => {
            deploy::sync_dirs(&dev)?;
        }
    }

    Ok(())
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
