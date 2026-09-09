// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
};
use tokio::{fs, task};

use eyre::Result;
use log::warn;
use owo_colors::colors::Blue;

use flate2::bufread::GzDecoder;

use crate::{
    dirs,
    models::{PackageJsonBinOnly, Release, Resolution, Spec},
    util::{self, LogDisplay as _},
};

fn unpack(release: &Release, bytes: &[u8], dest: &Path) -> Result<()> {
    match release {
        Release::Npm(_) => tar::Archive::new(GzDecoder::new(bytes)).unpack(dest)?,
        Release::Github(_) => zip::ZipArchive::new(Cursor::new(bytes))?.extract(dest)?,
    }

    Ok(())
}

pub async fn fetch_version(
    spec: &Spec,
    resolution: &Resolution,
) -> Result<(PathBuf, HashMap<String, String>)> {
    let release = &resolution.release;

    let cache_versions_dir = dirs::cache().join("versions").join(spec.name.to_string());
    fs::create_dir_all(&cache_versions_dir).await?;

    let cache_dir = cache_versions_dir.join(release.version());

    if fs::metadata(&cache_dir).await.is_ok() {
        warn!(
            "{:#} is already cached, not fetching",
            release.log_display::<Blue>()
        );

        let bin = PackageJsonBinOnly::read(&cache_dir).await?;
        return Ok((cache_dir, bin));
    }

    let unpack_dir = tempfile::Builder::new()
        .prefix("moldau-tmp")
        .tempdir_in(dirs::cache())?;

    // Unpacking into a subdirectory keeps the temporary directory itself from becoming
    // the root that is moved into the cache.
    let unpack_target = unpack_dir.path().join("root");

    let bytes = util::download(&release.to_string(), release.url()).await?;

    release.verify(&bytes)?;

    let bytes = task::spawn_blocking({
        let release = release.clone();
        let unpack_target = unpack_target.clone();

        move || -> Result<Vec<u8>> {
            unpack(&release, &bytes, &unpack_target)?;
            Ok(bytes)
        }
    })
    .await??;

    let unpack_root = util::find_root(&unpack_target).await?;

    spec.verify_integrity(&bytes, &unpack_root, resolution)
        .await?;

    fs::rename(unpack_root, &cache_dir).await?;
    unpack_dir.close()?;

    let bin = PackageJsonBinOnly::read(&cache_dir).await?;
    Ok((cache_dir, bin))
}

pub async fn fetch_spec(spec: &Spec) -> Result<(PathBuf, HashMap<String, String>)> {
    let resolution = spec.resolve().await?;
    fetch_version(spec, &resolution).await
}
