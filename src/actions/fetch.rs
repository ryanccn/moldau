// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::fs;

use eyre::{Result, bail, eyre};
use log::{debug, warn};
use owo_colors::colors::Blue;

use flate2::bufread::GzDecoder;
use tempdir::TempDir;

use crate::{
    dirs,
    models::{NpmPackage, NpmVersion, PackageJsonBinOnly, Spec, SpecName, SpecVersion},
    util::{LogDisplay as _, download},
};

async fn find_root(path: &Path) -> Result<PathBuf> {
    let mut readdir = fs::read_dir(&path).await?;
    let mut entries = Vec::new();

    while let Some(entry) = readdir.next_entry().await? {
        entries.push(entry.path());
    }

    if entries.len() == 1 {
        Ok(entries[0].clone())
    } else {
        Ok(path.to_owned())
    }
}

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

    let cache_dir = cache_versions_dir.join(version.version.to_string());

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

    let bytes = download(&version.to_string(), &version.dist.tarball).await?;

    if let Err((expected, actual)) = version.integrity()?.check(&bytes) {
        bail!(
            "integrity (download) mismatched for {version} (expected: {expected:?}, actual: {actual:?})"
        );
    }

    debug!("integrity (download) verified for {version}");

    if spec.name != SpecName::Yarn {
        if let Some(spec_integrity) = spec.version.integrity()? {
            if let Err((expected, actual)) = spec_integrity.check(&bytes) {
                bail!(
                    "integrity (spec) mismatched for {spec} (expected: {expected:?}, actual: {actual:?})"
                );
            }
        }

        debug!("integrity (spec) verified for {spec}");
    }

    tar::Archive::new(GzDecoder::new(&bytes[..])).unpack(&unpack_dir)?;
    let unpack_root = find_root(unpack_dir.path()).await?;

    // This special handling of integrity verification for Yarn is inherited from
    // Corepack. Corepack downloads Yarn as a file rather than a package, and
    // calculates the hash from that file. We download the package, but calculate
    // the hash for the file anyway for the sake of compatibility.

    if spec.name == SpecName::Yarn {
        if let Some(spec_integrity) = spec.version.integrity()? {
            let bin_path = version
                .bin
                .get("yarn")
                .ok_or_else(|| eyre!("could not resolve yarn bin path in {version}"))?;

            let bin_contents = fs::read(unpack_root.join(bin_path)).await?;

            if let Err((expected, actual)) = spec_integrity.check(&bin_contents) {
                bail!(
                    "integrity (spec) mismatched for {spec} (expected: {expected:?}, actual: {actual:?})"
                );
            }

            debug!("integrity (spec) verified for {spec}");
        }
    }

    fs::rename(unpack_root, &cache_dir).await?;
    unpack_dir.close()?;

    Ok((cache_dir, version.bin.clone()))
}

pub async fn fetch_spec(spec: &Spec) -> Result<(PathBuf, HashMap<String, String>)> {
    let resolved_version = resolve(spec).await?;
    fetch_version(spec, &resolved_version).await
}
