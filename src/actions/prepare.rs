// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};
use tokio::fs;

use eyre::Result;
use log::warn;
use owo_colors::colors::Blue;

use crate::{
    actions::fetch_spec,
    dirs,
    models::{PackageJsonBinOnly, Spec, SpecVersion},
    util::LogDisplay as _,
};

pub async fn prepare(spec: &Spec) -> Result<(PathBuf, HashMap<String, String>)> {
    let cache_versions_dir = dirs::cache().join("versions").join(spec.name.to_string());

    let mut cached_ok_versions = BTreeSet::new();

    // There is no way of knowing if a cached version matches a dist tag
    if !spec.version.is_dist_tag() {
        let mut read_dir = fs::read_dir(&cache_versions_dir).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            if let Ok(this_version) = semver::Version::parse(&entry.file_name().to_string_lossy()) {
                if match &spec.version {
                    SpecVersion::Exact(version) => {
                        // `Version::cmp_precedence` discards build metadata, unlike `==`
                        this_version.cmp_precedence(version).is_eq()
                    }
                    SpecVersion::SemverReq(req) => req.matches(&this_version),
                    SpecVersion::DistTag(_) => false,
                } {
                    cached_ok_versions.insert(this_version);
                }
            }
        }
    }

    if let Some(cache_ok_version) = cached_ok_versions.last() {
        let cache_dir = cache_versions_dir.join(cache_ok_version.to_string());

        let package_json = fs::read(cache_dir.join("package.json")).await?;
        let PackageJsonBinOnly { bin } = serde_json::from_slice(&package_json)?;

        return Ok((cache_dir, bin));
    }

    warn!("fetching package manager {}", spec.log_display::<Blue>());

    let outcome = fetch_spec(spec).await?;
    Ok(outcome)
}
