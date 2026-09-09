// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};
use tokio::{fs, io};

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
        if let Some(spec) = &self.package_manager {
            let spec: Spec = spec.parse()?;

            if !spec.version.is_exact() {
                bail!("`packageManager` specified in package.json must be exact");
            }

            return Ok(Some(spec));
        }

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

            if !spec.version.is_exact() {
                bail!("`devEngines.packageManager` specified in package.json must be exact");
            }

            return Ok(Some(spec));
        }

        Ok(None)
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum PackageJsonBin {
    Path(String),
    Map(HashMap<String, String>),
}

#[derive(Deserialize, Clone, Debug)]
pub struct PackageJsonBinOnly {
    name: Option<String>,
    bin: Option<PackageJsonBin>,
}

impl PackageJsonBinOnly {
    fn into_bin(self) -> HashMap<String, String> {
        match self.bin {
            Some(PackageJsonBin::Map(map)) => map,

            Some(PackageJsonBin::Path(path)) => match self.name {
                // A string `bin` is keyed by the package name, without its scope.
                Some(name) => {
                    let key = name.rsplit('/').next().unwrap_or(&name).to_owned();
                    HashMap::from([(key, path)])
                }
                None => HashMap::new(),
            },

            None => HashMap::new(),
        }
    }

    /// Reads the binaries provided by a package, which is empty when there is no manifest.
    pub async fn read(dir: &Path) -> Result<HashMap<String, String>> {
        match fs::read(dir.join("package.json")).await {
            Ok(data) => Ok(serde_json::from_slice::<Self>(&data)?.into_bin()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(err) => Err(err.into()),
        }
    }
}
