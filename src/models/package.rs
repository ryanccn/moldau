// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use eyre::{Result, bail};
use serde::Deserialize;

use super::{Spec, SpecVersion};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    pub package_manager: Option<String>,
    pub dev_engines: Option<DevEngines>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DevEngines {
    pub package_manager: Option<DevEnginesPackageManager>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DevEnginesPackageManager {
    pub name: String,
    pub version: Option<String>,
}

impl PackageJson {
    pub fn spec(&self) -> Result<Option<Spec>> {
        if let Some(data) = &self
            .dev_engines
            .as_ref()
            .and_then(|v| v.package_manager.as_ref())
        {
            let spec = Spec {
                name: data.name.parse()?,
                version: match &data.version {
                    Some(s) => s.parse()?,
                    None => SpecVersion::default(),
                },
            };

            if spec.version.exact().is_none() {
                bail!("`devEngines.packageManager` specified in package.json must be exact");
            }

            return Ok(Some(spec));
        }

        if let Some(spec) = &self.package_manager {
            let spec: Spec = spec.parse()?;

            if spec.version.exact().is_none() {
                bail!("`packageManager` specified in package.json must be exact");
            }

            return Ok(Some(spec));
        }

        Ok(None)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct PackageJsonBinOnly {
    #[serde(default)]
    pub bin: HashMap<String, String>,
}
