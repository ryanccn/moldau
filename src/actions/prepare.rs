// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::PathBuf};

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
    // There is no way of knowing if a cached version matches a dist tag
    let cache_ok_version = if spec.version.is_dist_tag() {
        None
    } else {
        super::cached_versions(spec.name)
            .await?
            .into_iter()
            .rfind(|this_version| match &spec.version {
                SpecVersion::Exact(version) => {
                    // `Version::cmp_precedence` discards build metadata, unlike `==`
                    this_version.cmp_precedence(version).is_eq()
                }
                SpecVersion::SemverReq(req) => req.matches(this_version),
                SpecVersion::DistTag(_) => false,
            })
    };

    if let Some(cache_ok_version) = cache_ok_version {
        let cache_dir = dirs::versions(spec.name).join(cache_ok_version.to_string());

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
