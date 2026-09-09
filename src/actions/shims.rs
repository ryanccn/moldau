// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::{fs, io};

use eyre::{Result, bail};
use log::{info, warn};

use crate::models::SpecBin;

#[cfg(unix)]
fn moldau_path() -> Result<PathBuf> {
    let current_exe = env::current_exe()?.canonicalize()?;

    // Linking to the name on `PATH`, when it resolves to this executable, keeps the shims
    // working across in-place upgrades.
    if let Ok(which_result) = which::which_global("moldau")
        && which_result.canonicalize().is_ok_and(|p| p == current_exe)
    {
        return Ok(which_result);
    }

    Ok(current_exe)
}

#[cfg(unix)]
async fn write_shims(dest: &Path, force: bool) -> Result<()> {
    let moldau = moldau_path()?;

    for shim in SpecBin::VARIANTS {
        let shim_path = dest.join(shim.to_string());

        if force
            && let Err(err) = fs::remove_file(&shim_path).await
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err.into());
        }

        if let Err(err) = fs::symlink(&moldau, &shim_path).await {
            if err.kind() != io::ErrorKind::AlreadyExists {
                return Err(err.into());
            }

            if !fs::read_link(&shim_path).await.is_ok_and(|p| p == moldau) {
                bail!(
                    "{} already exists and does not point at {}; pass `--force` to overwrite it",
                    shim_path.display(),
                    moldau.display()
                );
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
async fn write_shim_file(path: &Path, contents: &str, force: bool) -> Result<()> {
    // `fs::write` truncates unconditionally, so an existing shim has to be checked for
    // explicitly.
    if !force
        && let Ok(existing) = fs::read_to_string(path).await
        && existing != contents
    {
        bail!(
            "{} already exists and was not written by this version of moldau; pass `--force` to overwrite it",
            path.display()
        );
    }

    fs::write(path, contents).await?;

    Ok(())
}

#[cfg(windows)]
async fn write_shims(dest: &Path, force: bool) -> Result<()> {
    for shim in SpecBin::VARIANTS {
        let shim_bash_path = dest.join(shim.to_string());
        let shim_cmd_path = shim_bash_path.with_extension("cmd");

        write_shim_file(
            &shim_bash_path,
            &format!(
                r#"#!/bin/bash
exec moldau exec {shim} -- "$@"
"#,
            ),
            force,
        )
        .await?;

        write_shim_file(
            &shim_cmd_path,
            &format!(
                r"@echo off
setlocal
moldau exec {shim} -- %*
"
            ),
            force,
        )
        .await?;
    }

    Ok(())
}

pub async fn shims(dest: &Path, force: bool) -> Result<()> {
    fs::create_dir_all(&dest).await?;

    write_shims(dest, force).await?;

    info!("installed shims into {}", dest.display());

    if !env::var_os("PATH").is_some_and(|s| env::split_paths(&s).any(|p| p == dest)) {
        warn!(
            "{} is not in PATH; add it to the front of PATH for installed shims to take precedence",
            dest.display()
        );
    }

    Ok(())
}
