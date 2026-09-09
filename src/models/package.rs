// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};
use tokio::{fs, io};

use eyre::{Result, bail};
use serde::Deserialize;

use super::{Spec, SpecName, SpecVersion};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    package_manager: Option<String>,
    dev_engines: Option<DevEngines>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DevEngines {
    package_manager: Option<DevEnginesPackageManagers>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum DevEnginesPackageManagers {
    One(DevEnginesPackageManager),
    Many(Vec<DevEnginesPackageManager>),
}

/// What a `devEngines.packageManager` entry asks for when its package manager is not cached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnFail {
    #[default]
    Download,
    Warn,
    Error,
    /// Leaves the entry to whichever one follows it.
    Ignore,
}

impl OnFail {
    fn parse(s: &str) -> Self {
        match s {
            "warn" => Self::Warn,
            "error" => Self::Error,
            "ignore" => Self::Ignore,
            _ => Self::Download,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ManifestSpec {
    pub spec: Spec,
    pub on_fail: OnFail,
}

/// Where the entry read by [`PackageJson::spec`] sits, so that it is the one written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevEnginesLocation {
    Object,
    Index(usize),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DevEnginesPackageManager {
    name: String,
    version: Option<String>,
    on_fail: Option<String>,
}

impl DevEnginesPackageManager {
    fn on_fail(&self) -> OnFail {
        self.on_fail
            .as_deref()
            .map_or_else(OnFail::default, OnFail::parse)
    }

    fn is_selectable(&self) -> bool {
        self.on_fail() != OnFail::Ignore && self.name.parse::<SpecName>().is_ok()
    }
}

impl PackageJson {
    fn selected_dev_engines(&self) -> Option<(DevEnginesLocation, &DevEnginesPackageManager)> {
        let package_manager = self.dev_engines.as_ref()?.package_manager.as_ref()?;

        match package_manager {
            DevEnginesPackageManagers::One(entry) => entry
                .is_selectable()
                .then_some((DevEnginesLocation::Object, entry)),

            DevEnginesPackageManagers::Many(entries) => entries
                .iter()
                .enumerate()
                .find(|(_, entry)| entry.is_selectable())
                .map(|(index, entry)| (DevEnginesLocation::Index(index), entry)),
        }
    }

    #[must_use]
    pub fn dev_engines_location(&self) -> Option<DevEnginesLocation> {
        self.selected_dev_engines().map(|(location, _)| location)
    }

    pub fn spec(&self) -> Result<Option<ManifestSpec>> {
        // `devEngines.packageManager` is what is written back when present, so it is read first.
        if let Some((_, entry)) = self.selected_dev_engines() {
            let version = match &entry.version {
                Some(version) => version.parse()?,
                None => SpecVersion::default(),
            };

            if !version.is_exact() {
                bail!("`devEngines.packageManager` specified in package.json must be exact");
            }

            return Ok(Some(ManifestSpec {
                spec: Spec {
                    name: entry.name.parse()?,
                    version,
                },
                on_fail: entry.on_fail(),
            }));
        }

        if let Some(package_manager) = &self.package_manager {
            let spec: Spec = package_manager.parse()?;

            if !spec.version.is_exact() {
                bail!("`packageManager` specified in package.json must be exact");
            }

            return Ok(Some(ManifestSpec {
                spec,
                on_fail: OnFail::default(),
            }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn parse(manifest: Value) -> PackageJson {
        serde_json::from_value(manifest).expect("deserialize the manifest")
    }

    fn spec_of(manifest: Value) -> Option<ManifestSpec> {
        parse(manifest).spec().expect("read the specification")
    }

    fn error_of(manifest: Value) -> String {
        parse(manifest)
            .spec()
            .expect_err("the specification should be rejected")
            .to_string()
    }

    fn is_readable(manifest: Value) -> bool {
        serde_json::from_value::<PackageJson>(manifest).is_ok()
    }

    #[test]
    fn a_package_manager_field_is_read() {
        let manifest = spec_of(json!({ "packageManager": "pnpm@10.0.0" }))
            .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "pnpm@10.0.0");
        assert_eq!(manifest.on_fail, OnFail::Download);
    }

    #[test]
    fn dev_engines_is_read_before_the_package_manager_field() {
        let manifest = spec_of(json!({
            "packageManager": "npm@11.0.0",
            "devEngines": { "packageManager": { "name": "pnpm", "version": "10.0.0" } },
        }))
        .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "pnpm@10.0.0");
    }

    #[test]
    fn the_first_usable_entry_of_a_list_is_read() {
        let manifest = spec_of(json!({
            "devEngines": {
                "packageManager": [
                    { "name": "pnpm", "version": "10.0.0" },
                    { "name": "yarn", "version": "4.5.0" },
                ],
            },
        }))
        .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "pnpm@10.0.0");
    }

    #[test]
    fn an_ignored_entry_is_passed_over() {
        let manifest = spec_of(json!({
            "devEngines": {
                "packageManager": [
                    { "name": "pnpm", "version": "10.0.0", "onFail": "ignore" },
                    { "name": "yarn", "version": "4.5.0" },
                ],
            },
        }))
        .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "yarn@4.5.0");
    }

    #[test]
    fn an_entry_naming_an_unknown_package_manager_is_passed_over() {
        let manifest = spec_of(json!({
            "devEngines": {
                "packageManager": [
                    { "name": "deno", "version": "2.0.0" },
                    { "name": "bun", "version": "1.2.0" },
                ],
            },
        }))
        .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "bun@1.2.0");
    }

    #[test]
    fn a_list_with_no_usable_entry_falls_back_to_the_package_manager_field() {
        let manifest = spec_of(json!({
            "packageManager": "npm@11.0.0",
            "devEngines": {
                "packageManager": [{ "name": "pnpm", "version": "10.0.0", "onFail": "ignore" }],
            },
        }))
        .expect("the manifest declares a package manager");

        assert_eq!(manifest.spec.to_string(), "npm@11.0.0");
    }

    #[test]
    fn on_fail_is_read_and_falls_back_to_download() {
        for (value, expected) in [
            ("download", OnFail::Download),
            ("warn", OnFail::Warn),
            ("error", OnFail::Error),
            ("nonsense", OnFail::Download),
        ] {
            let manifest = spec_of(json!({
                "devEngines": {
                    "packageManager": { "name": "pnpm", "version": "10.0.0", "onFail": value },
                },
            }))
            .expect("the manifest declares a package manager");

            assert_eq!(manifest.on_fail, expected, "onFail: {value}");
        }
    }

    #[test]
    fn a_manifest_declaring_nothing_is_read_as_nothing() {
        assert!(spec_of(json!({ "name": "a-package" })).is_none());
        assert!(spec_of(json!({ "devEngines": { "runtime": { "name": "node" } } })).is_none());
    }

    #[test]
    fn an_inexact_version_is_rejected() {
        assert!(error_of(json!({ "packageManager": "pnpm@^10" })).contains("must be exact"));
        assert!(
            error_of(json!({
                "devEngines": { "packageManager": { "name": "pnpm", "version": "^10" } },
            }))
            .contains("must be exact")
        );
    }

    #[test]
    fn a_malformed_declaration_is_rejected() {
        assert!(!is_readable(json!({ "packageManager": 10 })));
        assert!(!is_readable(json!({
            "devEngines": { "packageManager": { "name": "pnpm", "version": 10 } },
        })));
        assert!(!is_readable(
            json!({ "devEngines": { "packageManager": ["pnpm@10.0.0"] } })
        ));
        assert!(!is_readable(json!({ "devEngines": "pnpm" })));
    }

    #[test]
    fn the_entry_that_is_written_is_the_entry_that_is_read() {
        let manifest = parse(json!({
            "devEngines": {
                "packageManager": [
                    { "name": "pnpm", "version": "10.0.0", "onFail": "ignore" },
                    { "name": "deno", "version": "2.0.0" },
                    { "name": "yarn", "version": "4.5.0" },
                ],
            },
        }));

        assert_eq!(
            manifest.dev_engines_location(),
            Some(DevEnginesLocation::Index(2))
        );
        assert_eq!(
            manifest
                .spec()
                .expect("read the specification")
                .expect("the manifest declares a package manager")
                .spec
                .to_string(),
            "yarn@4.5.0"
        );

        let manifest = parse(json!({
            "devEngines": { "packageManager": { "name": "pnpm", "version": "10.0.0" } },
        }));

        assert_eq!(
            manifest.dev_engines_location(),
            Some(DevEnginesLocation::Object)
        );
    }

    #[test]
    fn no_entry_is_located_when_none_is_read() {
        let located = |manifest| parse(manifest).dev_engines_location();

        assert!(
            located(json!({
                "devEngines": {
                    "packageManager": [{ "name": "pnpm", "version": "10.0.0", "onFail": "ignore" }],
                },
            }))
            .is_none()
        );
        assert!(
            located(json!({
                "devEngines": { "packageManager": { "name": "deno", "version": "2.0.0" } },
            }))
            .is_none()
        );
        assert!(located(json!({ "devEngines": { "runtime": { "name": "node" } } })).is_none());
    }
}
