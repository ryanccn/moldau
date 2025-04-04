// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::builder::PossibleValue;
use eyre::{Result, eyre};
use std::{env, fmt, iter, path::Path, str::FromStr};
use tokio::fs;

use super::PackageJson;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub name: SpecName,
    pub version: SpecVersion,
}

impl Spec {
    pub async fn parse(traverse: bool) -> Result<Option<Self>> {
        let cwd = env::current_dir()?;

        let path_iter: Box<dyn Iterator<Item = &Path>> = if traverse {
            Box::new(cwd.ancestors())
        } else {
            Box::new(iter::once(cwd.as_ref()))
        };

        for ancestor in path_iter {
            if let Some(data) = fs::read(ancestor.join("package.json"))
                .await
                .ok()
                .and_then(|d| serde_json::from_slice::<PackageJson>(&d).ok())
            {
                if let Some(spec) = data.spec()? {
                    return Ok(Some(spec));
                }
            }
        }

        Ok(None)
    }

    #[must_use]
    pub fn to_npm_package_name(&self) -> String {
        match self.name {
            SpecName::Npm => "npm".into(),

            SpecName::Yarn => {
                let is_classic = self.version.exact().is_some_and(|v| v.major <= 2)
                    || self.version.semver_req().is_some_and(|r| {
                        r.comparators.iter().any(|c| match c.op {
                            semver::Op::Exact
                            | semver::Op::LessEq
                            | semver::Op::Tilde
                            | semver::Op::Caret => c.major <= 1,
                            semver::Op::Less => {
                                c.major <= 1
                                    || c.major == 2
                                        && c.minor.is_none_or(|n| n == 0)
                                        && c.patch.is_none_or(|n| n == 0)
                            }
                            _ => false,
                        })
                    });

                if is_classic {
                    "yarn".into()
                } else {
                    "@yarnpkg/cli-dist".into()
                }
            }

            SpecName::Pnpm => "pnpm".into(),
        }
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{}@{:#}", self.name, self.version)
        } else {
            write!(f, "{}@{}", self.name, self.version)
        }
    }
}

impl FromStr for Spec {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.splitn(2, '@');

        let name = parts
            .next()
            .ok_or_else(|| eyre!("failed to obtain name from `packageManager`"))?
            .parse::<SpecName>()?;

        let version = match parts.next() {
            Some(s) => s.parse::<SpecVersion>()?,
            None => SpecVersion::default(),
        };

        Ok(Self { name, version })
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum SpecName {
    Npm,
    Yarn,
    Pnpm,
}

impl SpecName {
    pub const VARIANTS: &[Self] = &[Self::Npm, Self::Yarn, Self::Pnpm];
}

impl clap::ValueEnum for SpecName {
    fn value_variants<'a>() -> &'a [Self] {
        Self::VARIANTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecVersion {
    Exact(semver::Version),
    SemverReq(semver::VersionReq),
    DistTag(String),
}

impl SpecVersion {
    #[must_use]
    pub fn exact(&self) -> Option<&semver::Version> {
        if let Self::Exact(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[must_use]
    pub fn semver_req(&self) -> Option<&semver::VersionReq> {
        if let Self::SemverReq(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl fmt::Display for SpecVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&match self {
            Self::Exact(version) => {
                if f.alternate() {
                    let mut version = version.clone();
                    version.build = semver::BuildMetadata::EMPTY;
                    version.to_string()
                } else {
                    version.to_string()
                }
            }

            Self::SemverReq(req) => req.to_string(),

            Self::DistTag(tag) => tag.to_string(),
        })
    }
}

impl FromStr for SpecVersion {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self> {
        if let Ok(version) = semver::Version::parse(s) {
            return Ok(Self::Exact(version));
        }

        if let Ok(version_req) = semver::VersionReq::parse(s) {
            return Ok(Self::SemverReq(version_req));
        }

        Ok(Self::DistTag(s.to_owned()))
    }
}

impl Default for SpecVersion {
    fn default() -> Self {
        Self::SemverReq(semver::VersionReq::STAR)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecVersionIntegrity {
    SHA512(Vec<u8>),
    SHA1(Vec<u8>),
}

impl SpecVersionIntegrity {
    pub fn parse(s: &str) -> Result<Option<Self>> {
        Ok(if let Some(sha512) = s.strip_prefix("sha512.") {
            Some(Self::SHA512(hex::decode(sha512)?))
        } else if let Some(sha1) = s.strip_prefix("sha1.") {
            Some(Self::SHA1(hex::decode(sha1)?))
        } else {
            None
        })
    }

    #[must_use]
    pub fn check(&self, bytes: &[u8]) -> bool {
        match self {
            SpecVersionIntegrity::SHA512(expected) => {
                use sha2::{Digest as _, Sha512};
                expected == &Sha512::digest(bytes).to_vec()
            }
            SpecVersionIntegrity::SHA1(expected) => {
                use sha1_checked::{Digest as _, Sha1};
                expected == &Sha1::digest(bytes).to_vec()
            }
        }
    }
}

impl fmt::Display for SpecVersionIntegrity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SHA512(sha512) => write!(f, "sha512.{}", hex::encode(sha512)),
            Self::SHA1(sha1) => write!(f, "sha1.{}", hex::encode(sha1)),
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum SpecBin {
    Npm,
    Npx,
    Yarn,
    Yarnpkg,
    Pnpm,
    Pnpx,
}

impl SpecBin {
    pub const VARIANTS: &[Self] = &[
        Self::Npm,
        Self::Npx,
        Self::Yarn,
        Self::Yarnpkg,
        Self::Pnpm,
        Self::Pnpx,
    ];

    pub fn to_name(self) -> SpecName {
        match self {
            Self::Npm | Self::Npx => SpecName::Npm,
            Self::Yarn | Self::Yarnpkg => SpecName::Yarn,
            Self::Pnpm | Self::Pnpx => SpecName::Pnpm,
        }
    }
}

impl clap::ValueEnum for SpecBin {
    fn value_variants<'a>() -> &'a [Self] {
        Self::VARIANTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_string()))
    }
}

macro_rules! impl_fromstr_display {
    ($enum:ident, $($member:ident = $string:expr),* $(,)?) => {
        impl FromStr for $enum {
            type Err = eyre::Report;

            fn from_str(s: &str) -> Result<Self> {
                match s {
                    $($string => Ok(Self::$member),)*
                    &_ => Err(eyre!("invalid {}: {s:?}", stringify!($enum))),
                }
            }
        }

        impl fmt::Display for $enum {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self {
                    $(Self::$member => $string,)*
                })
            }
        }
    };
}

impl_fromstr_display! {
    SpecName,
    Npm = "npm",
    Yarn = "yarn",
    Pnpm = "pnpm",
}

impl_fromstr_display! {
    SpecBin,
    Npm = "npm",
    Npx = "npx",
    Yarn = "yarn",
    Yarnpkg = "yarnpkg",
    Pnpm = "pnpm",
    Pnpx = "pnpx",
}
