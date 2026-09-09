// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, ffi::OsString};
use tokio::process::Command;

use eyre::{Result, eyre};
use log::error;
use owo_colors::colors::Red;

use crate::{
    models::{OnFail, Spec, SpecBin, SpecName, SpecVersion},
    util::{ExitCodeError, LogDisplay as _},
};

pub async fn exec(bin: SpecBin, args: &[OsString], spec: Option<&Spec>) -> Result<bool> {
    let bin_default_spec = Spec {
        name: bin.to_name(),
        version: SpecVersion::default(),
    };

    let (mut spec, mut on_fail) = match spec {
        Some(v) => (v.to_owned(), OnFail::default()),
        None => match Spec::parse(true, Some(bin_default_spec.name)).await? {
            Some(manifest) => (manifest.spec, manifest.on_fail),
            None => (bin_default_spec.clone(), OnFail::default()),
        },
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

        let first_arg = args.first().and_then(|arg| arg.to_str());

        let transparent = bin == SpecBin::Npm
            || bin == SpecBin::Npx
            || bin == SpecBin::Pnpx
            || bin == SpecBin::Pnx
            || bin == SpecBin::Bunx
            || first_arg == Some("init")
            || (bin_default_spec.name == SpecName::Yarn || bin_default_spec.name == SpecName::Pnpm)
                && first_arg == Some("dlx");

        if disable_strict || transparent {
            spec = bin_default_spec;
            on_fail = OnFail::default();
        } else {
            error!(
                "{} is not available in the configured package manager {}",
                bin.log_display::<Red>(),
                spec.log_display::<Red>()
            );

            return Ok(false);
        }
    }

    let (cache_path, bins) = super::prepare(&spec, on_fail).await?;

    let mut command = if bins.is_empty() {
        let mut command = Command::new(cache_path.join(spec.name.standalone_bin()));
        command.args(bin.to_args()).args(args);
        command
    } else {
        let bin_path = bins
            .get(&bin.to_string())
            .ok_or_else(|| eyre!("could not obtain path of {bin:?} in {spec}"))?;

        let mut command = Command::new("node");
        command.arg(cache_path.join(bin_path)).args(args);
        command
    };

    if spec.name == SpecName::Pnpm {
        // Otherwise pnpm fetches the pinned version itself, on top of the one prepared here.
        command.env("PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS", "false");
    }

    let status = command.status().await?;

    if !status.success() {
        let code: u8 = status.code().and_then(|c| c.try_into().ok()).unwrap_or(1);
        return Err(ExitCodeError::from(code).into());
    }

    Ok(true)
}
