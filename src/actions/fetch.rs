// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::PathBuf};
use tokio::fs;

use eyre::{Result, bail};
use log::warn;
use owo_colors::colors::Blue;

use flate2::bufread::GzDecoder;
use tempdir::TempDir;

use crate::{
    dirs,
    models::{NpmPackage, NpmVersion, PackageJsonBinOnly, Spec, SpecVersion},
    util::{self, LogDisplay as _},
};

async fn resolve(spec: &Spec) -> Result<NpmVersion> {
    match &spec.version {
        SpecVersion::Exact(_) => {
            let version_data = NpmVersion::fetch(spec).await?;
            Ok(version_data)
        }

        SpecVersion::SemverReq(req) => {
            let package = NpmPackage::fetch(spec).await?;

            let Some(matching_version) = package.find_version_req(req) else {
                bail!("could not find matching version for {spec}");
            };

            Ok(matching_version)
        }

        SpecVersion::DistTag(tag) => {
            let package = NpmPackage::fetch(spec).await?;

            let Some(matching_version) = package.find_dist_tag(tag) else {
                bail!("could not find matching version for {spec}");
            };

            Ok(matching_version)
        }
    }
}

pub async fn fetch_version(
    spec: &Spec,
    version: &NpmVersion,
) -> Result<(PathBuf, HashMap<String, String>)> {
    let cache_versions_dir = dirs::cache().join("versions").join(spec.name.to_string());
    fs::create_dir_all(&cache_versions_dir).await?;

    let cache_dir = cache_versions_dir.join(&version.version);

    if cache_dir.exists() {
        warn!(
            "{:#} is already cached, not fetching",
            version.log_display::<Blue>()
        );

        let package_json = fs::read(cache_dir.join("package.json")).await?;
        let PackageJsonBinOnly { bin } = serde_json::from_slice(&package_json)?;

        return Ok((cache_dir, bin));
    }

    let unpack_dir = TempDir::new_in(dirs::cache(), "moldau-tmp")?;

    let bytes = util::download(&version.to_string(), &version.dist.tarball).await?;

    version.verify_integrity(&bytes)?;
    version.verify_signature()?;

    tar::Archive::new(GzDecoder::new(&bytes[..])).unpack(&unpack_dir)?;
    let unpack_root = util::find_root(unpack_dir.path()).await?;

    spec.verify_integrity(&bytes, &unpack_root, version).await?;

    fs::rename(unpack_root, &cache_dir).await?;
    unpack_dir.close()?;

    Ok((cache_dir, version.bin.clone()))
}

pub async fn fetch_spec(spec: &Spec) -> Result<(PathBuf, HashMap<String, String>)> {
    let resolved_version = resolve(spec).await?;
    fetch_version(spec, &resolved_version).await
}
