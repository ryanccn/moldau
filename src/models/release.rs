// SPDX-FileCopyrightText: 2026 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, fmt};

use eyre::Result;

use super::{GithubReleaseAsset, NpmVersion, SpecVersionIntegrity};

#[derive(Clone, Debug)]
pub enum Release {
    Npm(NpmVersion),
    Github(GithubReleaseAsset),
}

impl Release {
    #[must_use]
    pub fn version(&self) -> &str {
        match self {
            Self::Npm(version) => &version.version,
            Self::Github(asset) => &asset.version,
        }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        match self {
            Self::Npm(version) => &version.dist.tarball,
            Self::Github(asset) => &asset.url,
        }
    }

    /// The binaries provided by this release, which is empty for standalone executables.
    #[must_use]
    pub fn bin(&self) -> HashMap<String, String> {
        match self {
            Self::Npm(version) => version.bin.clone(),
            Self::Github(_) => HashMap::new(),
        }
    }

    #[must_use]
    pub fn is_platform_specific(&self) -> bool {
        matches!(self, Self::Github(_))
    }

    pub fn integrity(&self) -> Result<Option<SpecVersionIntegrity>> {
        match self {
            Self::Npm(version) => version.integrity().map(Some),
            Self::Github(asset) => asset.integrity(),
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<()> {
        match self {
            Self::Npm(version) => {
                version.verify_integrity(bytes)?;
                version.verify_signature()
            }
            Self::Github(asset) => asset.verify_integrity(bytes),
        }
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Npm(version) => version.fmt(f),
            Self::Github(asset) => asset.fmt(f),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Resolution {
    pub release: Release,
    /// The release that was resolved, when it is not the one that is fetched.
    pub resolved: Option<Release>,
}

impl Resolution {
    /// The release whose integrity `packageManager` records.
    #[must_use]
    pub fn pinned(&self) -> &Release {
        self.resolved.as_ref().unwrap_or(&self.release)
    }
}
