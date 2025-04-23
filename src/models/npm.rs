// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, env, fmt, sync::LazyLock};

use base64::prelude::{BASE64_STANDARD, Engine as _};
use eyre::{Result, eyre};
use log::debug;
use reqwest::header;
use serde::Deserialize;

use super::{Spec, SpecVersionIntegrity};
use crate::http::HTTP;

static NPM_REGISTRY: LazyLock<String> = LazyLock::new(|| {
    env::var("COREPACK_NPM_REGISTRY").unwrap_or_else(|_| "https://registry.npmjs.org".to_owned())
});

static NPM_INSTALL_HEADER_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct NpmPackage {
    pub versions: HashMap<String, NpmVersion>,
    pub dist_tags: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct NpmVersion {
    pub name: String,
    pub version: semver::Version,
    #[serde(default)]
    pub bin: HashMap<String, String>,
    pub dist: NpmVersionDist,
}

impl fmt::Display for NpmVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct NpmVersionDist {
    pub tarball: String,
    pub shasum: String,
    pub integrity: Option<String>,
}

impl NpmPackage {
    pub async fn fetch(spec: &Spec) -> Result<Self> {
        let mut url = reqwest::Url::parse(&NPM_REGISTRY)?;
        url.path_segments_mut()
            .map_err(|()| eyre!("failed to construct npm registry URL"))?
            .push(&spec.to_npm_package_name());

        debug!("fetching npm package: {url}");

        Ok(HTTP
            .get(url)
            .header(header::ACCEPT, NPM_INSTALL_HEADER_ACCEPT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    #[must_use]
    pub fn find_version_req(&self, req: &semver::VersionReq) -> Option<NpmVersion> {
        let mut parsed_versions = self
            .versions
            .iter()
            .filter_map(|(k, v)| semver::Version::parse(k).ok().map(|s| (s, v)))
            .filter(|(k, _)| req.matches(k))
            .collect::<Vec<_>>();

        parsed_versions.sort_unstable_by(|a, b| a.0.cmp_precedence(&b.0));
        parsed_versions.last().map(|a| a.1).cloned()
    }

    #[must_use]
    pub fn find_dist_tag(&self, dist_tag: &str) -> Option<NpmVersion> {
        self.dist_tags
            .get(dist_tag)
            .and_then(|v| self.versions.get(v.as_str()))
            .cloned()
    }
}

impl NpmVersion {
    pub async fn fetch(spec: &Spec) -> Result<Self> {
        let mut url = reqwest::Url::parse(&NPM_REGISTRY)?;
        url.path_segments_mut()
            .map_err(|()| eyre!("failed to construct npm registry URL"))?
            .push(&spec.to_npm_package_name())
            .push(&format!("{:#}", spec.version));

        debug!("fetching npm version: {url}");

        Ok(HTTP
            .get(url)
            .header(header::ACCEPT, NPM_INSTALL_HEADER_ACCEPT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub fn integrity(&self) -> Result<SpecVersionIntegrity> {
        if let Some(integrity) = &self.dist.integrity {
            let sha512 = BASE64_STANDARD.decode(
                integrity
                    .strip_prefix("sha512-")
                    .ok_or_else(|| eyre!("unexpected format in npm integrity: {integrity:?}"))?,
            )?;

            Ok(SpecVersionIntegrity::SHA512(sha512))
        } else {
            Ok(SpecVersionIntegrity::SHA1(hex::decode(&self.dist.shasum)?))
        }
    }
}
