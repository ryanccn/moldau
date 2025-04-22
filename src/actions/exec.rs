// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use tokio::process::Command;

use eyre::{Result, eyre};
use log::error;
use owo_colors::colors::Red;

use crate::{
    models::{Spec, SpecBin, SpecName, SpecVersion},
    util::{ExitCodeError, LogDisplay as _},
};

pub async fn exec(bin: SpecBin, args: &[String], spec: Option<&Spec>) -> Result<bool> {
    let bin_default_spec = Spec {
        name: bin.to_name(),
        version: SpecVersion::default(),
    };

    let mut spec = match spec {
        Some(v) => v.to_owned(),
        None => Spec::parse(true)
            .await?
            .unwrap_or_else(|| bin_default_spec.clone()),
    };

    if spec.name != bin_default_spec.name {
        let disable_strict = env::var("COREPACK_ENABLE_STRICT").is_ok_and(|s| s == "0");

        // "Transparent" commands, as specified by Corepack, are commands that are allowed
        // to be run regardless of the current project's package manager spec. This includes
        // commands for one-off execution (e.g. `npx`, `pnpm dlx`) and project initialization.
        // The detection mechanism (via arguments) is not quite reliable, but it should cover
        // the majority of use cases, and in the interest of compatibility we support this.
        //
        // We also consider `npm` a transparent command, because `npm` is typically not managed
        // by Corepack and some Node.js projects assume `npm` availability regardless of the
        // currently configured package manager (since it is bundled with the Node.js
        // distribution, after all). Enforcing strictness for `npm` would break these projects.

        let transparent = bin == SpecBin::Npm
            || bin == SpecBin::Npx
            || bin == SpecBin::Pnpx
            || args.first().is_some_and(|s| s == "init")
            || (bin_default_spec.name == SpecName::Yarn || bin_default_spec.name == SpecName::Pnpm)
                && args.first().is_some_and(|s| s == "dlx");

        if disable_strict || transparent {
            spec = bin_default_spec;
        } else {
            error!(
                "{} is not available in the configured package manager {:#}",
                bin.log_display::<Red>(),
                spec.log_display::<Red>()
            );

            return Ok(false);
        }
    }

    let (cache_path, bins) = super::prepare(&spec).await?;

    let bin_path = bins
        .get(&bin.to_string())
        .ok_or_else(|| eyre!("could not obtain path of {bin:?} in {spec}"))?;

    let status = Command::new(cache_path.join(bin_path))
        .args(args)
        .status()
        .await?;

    if !status.success() {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Err(ExitCodeError::from(status.code().unwrap_or(1) as u8).into());
    }

    Ok(true)
}
