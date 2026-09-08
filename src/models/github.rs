// SPDX-FileCopyrightText: 2026 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, fmt};

use eyre::{Result, bail, eyre};
use log::debug;
use reqwest::{
    StatusCode, Url,
    header::{self, HeaderMap, HeaderValue},
};
use serde::{Deserialize, de::DeserializeOwned};

use super::{SpecVersion, SpecVersionIntegrity};
use crate::http::HTTP;

static GITHUB_API: &str = "https://api.github.com";

fn github_common_headers() -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, "application/vnd.github+json".parse()?);

    if let Ok(token) = env::var("GITHUB_TOKEN") {
        let mut header: HeaderValue = format!("Bearer {token}").parse()?;
        header.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, header);
    }

    Ok(headers)
}

/// Fetches from the GitHub API, treating a missing resource as an absent result.
async fn fetch<T: DeserializeOwned>(url: Url) -> Result<Option<T>> {
    debug!("fetching GitHub API: {url}");

    let resp = HTTP
        .get(url)
        .headers(github_common_headers()?)
        .send()
        .await?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    Ok(Some(resp.error_for_status()?.json().await?))
}

#[derive(Clone, Debug)]
pub struct GithubSource {
    pub repo: &'static str,
    pub tag_prefix: &'static str,
    pub asset: String,
}

#[derive(Deserialize, Clone, Debug)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize, Clone, Debug)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GithubReleaseAsset {
    pub repo: &'static str,
    pub version: String,
    pub url: String,
    pub digest: Option<String>,
}

impl GithubSource {
    fn url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = Url::parse(GITHUB_API)?;

        url.path_segments_mut()
            .map_err(|()| eyre!("failed to construct GitHub API URL"))?
            .push("repos")
            .extend(self.repo.split('/'))
            .push("releases")
            .extend(segments);

        Ok(url)
    }

    fn tag_version(&self, tag: &str) -> Option<semver::Version> {
        semver::Version::parse(tag.strip_prefix(self.tag_prefix)?).ok()
    }

    pub async fn resolve(&self, version: &SpecVersion) -> Result<Option<GithubReleaseAsset>> {
        let release: Option<GithubRelease> = match version {
            SpecVersion::Exact(_) => {
                let tag = format!("{}{}", self.tag_prefix, version.to_plain_string());
                fetch(self.url(&["tags", &tag])?).await?
            }

            // GitHub releases have no equivalent of dist tags apart from the latest
            // published stable release.
            SpecVersion::DistTag(tag) => {
                if tag == "latest" {
                    fetch(self.url(&["latest"])?).await?
                } else {
                    None
                }
            }

            // Releases are returned newest first, so the highest matching version is
            // expected to be on the first page.
            SpecVersion::SemverReq(req) => {
                let mut url = self.url(&[])?;
                url.query_pairs_mut().append_pair("per_page", "100");

                fetch::<Vec<GithubRelease>>(url)
                    .await?
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|release| Some((self.tag_version(&release.tag_name)?, release)))
                    .filter(|(version, _)| req.matches(version))
                    .max_by(|a, b| a.0.cmp_precedence(&b.0))
                    .map(|(_, release)| release)
            }
        };

        release.map(|release| self.asset(release)).transpose()
    }

    fn asset(&self, release: GithubRelease) -> Result<GithubReleaseAsset> {
        let Some(version) = self.tag_version(&release.tag_name) else {
            bail!(
                "unexpected release tag in {}: {:?}",
                self.repo,
                release.tag_name
            );
        };

        let Some(asset) = release
            .assets
            .into_iter()
            .find(|asset| asset.name == self.asset)
        else {
            bail!("{} {version} does not provide {}", self.repo, self.asset);
        };

        Ok(GithubReleaseAsset {
            repo: self.repo,
            version: version.to_string(),
            url: asset.browser_download_url,
            digest: asset.digest,
        })
    }
}

impl GithubReleaseAsset {
    pub fn integrity(&self) -> Result<Option<SpecVersionIntegrity>> {
        self.digest
            .as_deref()
            .map(|digest| {
                let hash = digest
                    .strip_prefix("sha256:")
                    .ok_or_else(|| eyre!("unexpected format in GitHub digest: {digest:?}"))?;

                Ok(SpecVersionIntegrity::sha256(hex::decode(hash)?))
            })
            .transpose()
    }

    pub fn verify_integrity(&self, bytes: &[u8]) -> Result<()> {
        // Not all release assets have a recorded digest.
        let Some(integrity) = self.integrity()? else {
            debug!("skipped integrity verification for {self} (no digest provided)");
            return Ok(());
        };

        if let Err((expected, actual)) = integrity.verify(bytes) {
            bail!(
                "integrity (download) failed to verify for {self} (expected: {expected}, actual: {actual})"
            );
        }

        debug!("integrity (download) verified for {self}");
        Ok(())
    }
}

impl fmt::Display for GithubReleaseAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.repo, self.version)
    }
}
