// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, path::Path};
use tokio::{fs, io};

use eyre::{Context, Result};
use log::{info, warn};
use which::which_global;

use crate::models::SpecBin;

pub async fn shims(dest: &Path, force: bool) -> Result<()> {
    let dest = dest.canonicalize()?;

    fs::create_dir_all(&dest).await?;
    let bin_path = which_global("moldau").wrap_err("could not resolve moldau binary in PATH")?;

    for bin in SpecBin::VARIANTS {
        #[cfg(unix)]
        let shim_path = dest.join(bin.to_string());
        #[cfg(windows)]
        let shim_path = dest.join(format!("{bin}.exe"));

        if force {
            if let Err(err) = fs::remove_file(&shim_path).await {
                if err.kind() != io::ErrorKind::AlreadyExists {
                    return Err(err.into());
                }
            }
        }

        #[cfg(unix)]
        let outcome = fs::symlink(&bin_path, &shim_path).await;
        #[cfg(windows)]
        let outcome = fs::hard_link(&bin_path, &shim_path).await;

        if let Err(err) = outcome {
            if err.kind() == io::ErrorKind::AlreadyExists {
                if !fs::read_link(&shim_path).await.is_ok_and(|p| p == bin_path) {
                    return Err(err.into());
                }
            } else {
                return Err(err.into());
            }
        }
    }

    info!("installed shims into {}", dest.display());

    if !env::var_os("PATH").is_some_and(|s| env::split_paths(&s).any(|p| p == dest)) {
        warn!(
            "{} is not in PATH; add it to the front of PATH for installed shims to take precedence",
            dest.display()
        );
    }

    Ok(())
}
