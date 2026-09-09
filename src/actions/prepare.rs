// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};
use tokio::fs;

use eyre::{Result, bail};
use log::{info, warn};
use owo_colors::colors::{Blue, Yellow};

use crate::{
    actions::fetch_spec,
    dirs,
    models::{OnFail, PackageJsonBinOnly, Spec, SpecVersion},
    util::LogDisplay as _,
};

pub async fn prepare(spec: &Spec, on_fail: OnFail) -> Result<(PathBuf, HashMap<String, String>)> {
    let cache_versions_dir = dirs::cache().join("versions").join(spec.name.to_string());

    let mut cached_ok_versions = BTreeSet::new();

    // There is no way of knowing if a cached version matches a dist tag
    if !spec.version.is_dist_tag()
        && let Ok(mut read_dir) = fs::read_dir(&cache_versions_dir).await
    {
        while let Some(entry) = read_dir.next_entry().await? {
            if let Ok(this_version) = semver::Version::parse(&entry.file_name().to_string_lossy())
                && match &spec.version {
                    SpecVersion::Exact(version) => {
                        // `Version::cmp_precedence` discards build metadata, unlike `==`
                        this_version.cmp_precedence(version).is_eq()
                    }
                    SpecVersion::SemverReq(req) => req.matches(&this_version),
                    SpecVersion::DistTag(_) => false,
                }
            {
                cached_ok_versions.insert(this_version);
            }
        }
    }

    if let Some(cache_ok_version) = cached_ok_versions.last() {
        let cache_dir = cache_versions_dir.join(cache_ok_version.to_string());

        let bin = PackageJsonBinOnly::read(&cache_dir).await?;
        return Ok((cache_dir, bin));
    }

    match on_fail {
        OnFail::Error => {
            bail!(
                "{spec} is not cached, and `onFail` in `devEngines.packageManager` is set to `error`; run `moldau prefetch` to fetch it"
            );
        }
        OnFail::Warn => {
            warn!("{} is not cached", spec.log_display::<Yellow>());
        }
        OnFail::Download | OnFail::Ignore => {}
    }

    info!("fetching package manager {}", spec.log_display::<Blue>());

    fetch_spec(spec).await
}
