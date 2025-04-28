// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::builder::PossibleValue;
use eyre::{Result, bail, eyre};
use log::debug;

use std::{
    env, fmt, iter,
    path::{self, Path},
    str::FromStr,
};
use tokio::fs;

use super::{NpmVersion, PackageJson};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub name: SpecName,
    pub version: SpecVersion,
}

enum SpecPathIterator<'a> {
    Traverse(path::Ancestors<'a>),
    NoTraverse(iter::Once<&'a Path>),
}

impl<'a> Iterator for SpecPathIterator<'a> {
    type Item = &'a Path;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Traverse(it) => it.next(),
            Self::NoTraverse(it) => it.next(),
        }
    }
}

impl Spec {
    pub async fn parse(traverse: bool) -> Result<Option<Self>> {
        let cwd = env::current_dir()?;

        for ancestor in if traverse {
            SpecPathIterator::Traverse(cwd.ancestors())
        } else {
            SpecPathIterator::NoTraverse(iter::once(cwd.as_ref()))
        } {
            if let Some(data) = fs::read(ancestor.join("package.json"))
                .await
                .ok()
                .and_then(|d| serde_json::from_slice::<PackageJson>(&d).ok())
            {
                if let Some(spec) = data.spec()? {
                    debug!("parsed spec from {}: {spec}", ancestor.display());
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
                let is_classic = match &self.version {
                    SpecVersion::Exact(v) => v.major <= 1,
                    SpecVersion::SemverReq(r) => r.comparators.iter().any(|c| match c.op {
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
                    }),
                    SpecVersion::DistTag(_) => false,
                };

                if is_classic {
                    "yarn".into()
                } else {
                    "@yarnpkg/cli-dist".into()
                }
            }

            SpecName::Pnpm => "pnpm".into(),
        }
    }

    pub async fn verify_integrity(
        &self,
        bytes: &[u8],
        unpack_root: &Path,
        version: &NpmVersion,
    ) -> Result<()> {
        // This special handling of integrity verification for Yarn is inherited from
        // Corepack. Corepack downloads Yarn as a file rather than a package, and
        // calculates the hash from that file. We download the package, but calculate
        // the hash for the file anyway for the sake of compatibility.

        if self.name == SpecName::Yarn {
            if let Some(integrity) = self.version.integrity()? {
                let bin_path = version
                    .bin
                    .get("yarn")
                    .ok_or_else(|| eyre!("could not resolve yarn bin path in {version}"))?;

                let bin_contents = fs::read(unpack_root.join(bin_path)).await?;

                if let Err((expected, actual)) = integrity.verify(&bin_contents) {
                    bail!(
                        "integrity (spec) failed to verify for {self} (expected: {expected}, actual: {actual})"
                    );
                }

                debug!("integrity (spec) verified for {self}");
            }
        } else {
            if let Some(integrity) = self.version.integrity()? {
                if let Err((expected, actual)) = integrity.verify(bytes) {
                    bail!(
                        "integrity (spec) failed to verify for {self} (expected: {expected}, actual: {actual})"
                    );
                }
            }

            debug!("integrity (spec) verified for {self}");
        }

        Ok(())
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
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    #[must_use]
    pub fn is_dist_tag(&self) -> bool {
        matches!(self, Self::DistTag(_))
    }

    pub fn integrity(&self) -> Result<Option<SpecVersionIntegrity>> {
        match self {
            Self::Exact(v) => SpecVersionIntegrity::parse(&v.build),
            _ => Ok(None),
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
    SHA384(Vec<u8>),
    SHA256(Vec<u8>),
    SHA224(Vec<u8>),
    SHA1(Vec<u8>),
}

impl SpecVersionIntegrity {
    pub fn parse(s: &str) -> Result<Option<Self>> {
        if let Some(hash) = s.strip_prefix("sha512.") {
            Ok(Some(Self::SHA512(hex::decode(hash)?)))
        } else if let Some(hash) = s.strip_prefix("sha384.") {
            Ok(Some(Self::SHA384(hex::decode(hash)?)))
        } else if let Some(hash) = s.strip_prefix("sha256.") {
            Ok(Some(Self::SHA256(hex::decode(hash)?)))
        } else if let Some(hash) = s.strip_prefix("sha224.") {
            Ok(Some(Self::SHA224(hex::decode(hash)?)))
        } else if let Some(hash) = s.strip_prefix("sha1.") {
            Ok(Some(Self::SHA1(hex::decode(hash)?)))
        } else {
            Ok(None)
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<(), (String, String)> {
        use sha1_checked::Sha1;
        use sha2::{Digest as _, Sha224, Sha256, Sha384, Sha512};

        let expected: &Vec<u8>;
        let actual: Vec<u8>;

        match self {
            Self::SHA512(hash) => {
                expected = hash;
                actual = Sha512::digest(bytes).to_vec();
            }
            Self::SHA384(hash) => {
                expected = hash;
                actual = Sha384::digest(bytes).to_vec();
            }
            Self::SHA256(hash) => {
                expected = hash;
                actual = Sha256::digest(bytes).to_vec();
            }
            Self::SHA224(hash) => {
                expected = hash;
                actual = Sha224::digest(bytes).to_vec();
            }
            Self::SHA1(hash) => {
                expected = hash;
                actual = Sha1::digest(bytes).to_vec();
            }
        }

        if expected == &actual {
            Ok(())
        } else {
            Err((hex::encode(expected), hex::encode(actual)))
        }
    }
}

impl fmt::Display for SpecVersionIntegrity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SHA512(hash) => write!(f, "sha512.{}", hex::encode(hash)),
            Self::SHA384(hash) => write!(f, "sha384.{}", hex::encode(hash)),
            Self::SHA256(hash) => write!(f, "sha256.{}", hex::encode(hash)),
            Self::SHA224(hash) => write!(f, "sha224.{}", hex::encode(hash)),
            Self::SHA1(hash) => write!(f, "sha1.{}", hex::encode(hash)),
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
    ($enum:ident, $($member:ident = $string:expr),+ $(,)?) => {
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
