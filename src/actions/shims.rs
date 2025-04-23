// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, path::Path};
use tokio::{fs, io};

use eyre::Result;
use log::{info, warn};

use crate::models::SpecBin;

#[cfg(unix)]
async fn write_shim(dest: &Path, shim: &SpecBin, force: bool) -> Result<()> {
    use which::which_global;
    let moldau = which_global("moldau")?;

    let shim_path = dest.join(shim.to_string());

    if force {
        if let Err(err) = fs::remove_file(&shim_path).await {
            if err.kind() != io::ErrorKind::NotFound {
                return Err(err.into());
            }
        }
    }

    let outcome = fs::symlink(&moldau, &shim_path).await;

    if let Err(err) = outcome {
        if err.kind() == io::ErrorKind::AlreadyExists {
            if !fs::read_link(&shim_path).await.is_ok_and(|p| p == moldau) {
                return Err(err.into());
            }
        } else {
            return Err(err.into());
        }
    }

    Ok(())
}

#[cfg(windows)]
async fn write_shim(dest: &Path, shim: &SpecBin, force: bool) -> Result<()> {
    let shim_bash_path = dest.join(shim.to_string());
    let shim_cmd_path = shim_bash_path.with_extension("cmd");

    if force {
        if let Err(err) = fs::remove_file(&shim_bash_path).await {
            if err.kind() != io::ErrorKind::NotFound {
                return Err(err.into());
            }
        }

        if let Err(err) = fs::remove_file(&shim_cmd_path).await {
            if err.kind() != io::ErrorKind::NotFound {
                return Err(err.into());
            }
        }
    }

    fs::write(
        shim_bash_path,
        format!(
            r#"#!/bin/bash
exec moldau exec {shim} -- "$@"
"#,
        ),
    )
    .await?;

    fs::write(
        shim_cmd_path,
        format!(
            r"@echo off
setlocal
moldau exec {shim} -- %*
"
        ),
    )
    .await?;

    Ok(())
}

pub async fn shims(dest: &Path, force: bool) -> Result<()> {
    fs::create_dir_all(&dest).await?;

    for shim in SpecBin::VARIANTS {
        write_shim(dest, shim, force).await?;
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
