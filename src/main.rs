// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eyre::{Result, bail};
use std::{
    env,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory as _, Parser, Subcommand};
use log::info;
use owo_colors::{OwoColorize as _, colors::Blue};

mod actions;
mod dirs;
mod http;
mod models;
mod util;

use crate::{
    models::{Spec, SpecBin, SpecVersion},
    util::{ExitCodeError, LogDisplay as _, ToExitCode as _},
};

#[derive(Parser, Clone, Debug)]
#[command(version, about, long_about = None, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone, Debug)]
enum Commands {
    /// Execute a package manager
    Exec {
        /// Package manager binary to execute
        bin: SpecBin,

        /// Specification for the package manager
        #[clap(long)]
        spec: Option<Spec>,

        /// Arguments to pass to the package manager
        #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Use a package manager
    ///
    /// Sets to `packageManager` (default) or `devEngines.packageManager` (detected based on usage)
    Use {
        /// Specification for the package manager
        spec: Spec,

        /// Prefetch the specified package manager
        #[clap(long)]
        prefetch: bool,
    },

    /// Upgrade a package manager
    ///
    /// Reads from and sets to `packageManager` (default) or `devEngines.packageManager` (detected based on usage)
    Up {
        /// Prefetch the specified package manager
        #[clap(long)]
        prefetch: bool,
    },

    /// Prefetch a package manager
    ///
    /// Reads from `packageManager` or `devEngines.packageManager`, or takes an argument
    Prefetch {
        /// Specification for the package manager
        spec: Option<Spec>,
    },

    /// Install shims to a destination directory
    Shims {
        /// Directory to write shims into
        #[clap(default_value = dirs::data().join("shims").into_os_string())]
        dest: PathBuf,

        /// Overwrite shims if destination paths already exist
        #[clap(short, long)]
        force: bool,
    },

    /// Clean the package manager cache
    Clean {
        /// Remove the latest versions of package managers from the cache as well
        #[clap(short, long)]
        all: bool,
    },

    /// Generate shell completions
    Completions {
        /// The shell to generate completions for    
        shell: clap_complete::Shell,
    },
}

async fn main_fallible() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("moldau=info"))
        .format(|buf, record| {
            let level_style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "{}{}{}{:#}{} {}",
                "[".dimmed(),
                level_style,
                record.level(),
                level_style,
                "]".dimmed(),
                record.args()
            )
        })
        .init();

    color_eyre::install()?;

    let mut args = env::args();
    if let Some(bin) = args.next().and_then(|argv0| {
        Path::new(&argv0)
            .file_stem()
            .and_then(|stem| stem.to_string_lossy().parse::<SpecBin>().ok())
    }) {
        let success = actions::exec(bin, &args.collect::<Vec<_>>(), None).await?;

        if !success {
            return Err(ExitCodeError::FAILURE.into());
        }

        return Err(ExitCodeError::SUCCESS.into());
    }

    let cli = Cli::parse();

    match &cli.command {
        Commands::Exec { bin, args, spec } => {
            let success = actions::exec(*bin, &args[..], spec.as_ref()).await?;
            if !success {
                return Err(ExitCodeError::FAILURE.into());
            }
        }

        Commands::Use { spec, prefetch } => {
            actions::use_(spec).await?;

            if *prefetch {
                actions::fetch_spec(spec).await?;
            }
        }

        Commands::Up { prefetch } => {
            let Some(spec) = Spec::parse(false).await? else {
                bail!("no `packageManager` or `devEngines.packageManager` configured!");
            };

            let spec = Spec {
                name: spec.name,
                version: SpecVersion::default(),
            };

            actions::use_(&spec).await?;

            if *prefetch {
                actions::fetch_spec(&spec).await?;
            }
        }

        Commands::Prefetch { spec } => {
            let spec = match spec {
                Some(spec) => spec,
                None => &match Spec::parse(true).await? {
                    Some(spec) => spec,
                    None => bail!("no `packageManager` or `devEngines.packageManager` configured!"),
                },
            };

            info!("prefetching package manager {}", spec.log_display::<Blue>());

            actions::fetch_spec(spec).await?;
        }

        Commands::Shims { dest, force } => {
            actions::shims(dest, *force).await?;
        }

        Commands::Clean { all } => {
            actions::clean(*all).await?;
        }

        Commands::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "moldau", &mut io::stdout());
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    main_fallible().await.to_exit_code()
}
