// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::builder::PossibleValue;
use eyre::{Result, WrapErr as _, bail, eyre};
use log::{debug, warn};

use std::{
    borrow::Cow,
    env, fmt, iter,
    path::{self, Path},
    str::FromStr,
};
use tokio::{fs, task};

use crate::{models::NpmPackage, util};

use super::{GithubSource, ManifestSpec, NpmVersion, PackageJson, Release, Resolution};

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

#[derive(Clone, Debug)]
enum SpecSource {
    Npm(String),
    Github(GithubSource),
}

impl SpecSource {
    async fn resolve(&self, version: &SpecVersion) -> Result<Option<Release>> {
        Ok(match self {
            Self::Npm(package) => match version {
                // The registry resolves dist tags through this endpoint as well, which
                // avoids fetching the entire packument.
                SpecVersion::Exact(_) | SpecVersion::DistTag(_) => {
                    NpmVersion::fetch(package, &version.to_plain_string()).await?
                }
                SpecVersion::SemverReq(req) => {
                    NpmPackage::fetch(package).await?.find_version_req(req)
                }
            }
            .map(Release::Npm),

            Self::Github(source) => source.resolve(version).await?.map(Release::Github),
        })
    }
}

impl Spec {
    /// Reads the specification a manifest declares. `preferred` is the package manager
    /// being run, which a `devEngines.packageManager` list is read in favour of.
    pub async fn parse(
        traverse: bool,
        preferred: Option<SpecName>,
    ) -> Result<Option<ManifestSpec>> {
        let cwd = env::current_dir()?;

        for ancestor in if traverse {
            SpecPathIterator::Traverse(cwd.ancestors())
        } else {
            SpecPathIterator::NoTraverse(iter::once(cwd.as_ref()))
        } {
            let path = ancestor.join("package.json");

            let Ok(contents) = fs::read(&path).await else {
                continue;
            };

            // A file that does not hold JSON is not a manifest, but one that does is read
            // strictly.
            if serde_json::from_slice::<serde::de::IgnoredAny>(&contents).is_err() {
                debug!("skipping {}", path.display());
                continue;
            }

            let data = serde_json::from_slice::<PackageJson>(&contents)
                .wrap_err_with(|| format!("could not parse {}", path.display()))?;

            if let Some(manifest) = data.spec(preferred)? {
                debug!("parsed spec from {}: {}", path.display(), manifest.spec);
                return Ok(Some(manifest));
            }
        }

        Ok(None)
    }

    /// The sources that this specification is resolved through.
    fn sources(&self) -> Result<Vec<SpecSource>> {
        Ok(match self.name {
            SpecName::Npm => vec![SpecSource::Npm("npm".into())],

            SpecName::Yarn => {
                // A specification that is not constrained to a single major version range
                // can match a version from either source.

                let npm = SpecSource::Npm(
                    if self.version.is_below_major(2) {
                        "yarn"
                    } else {
                        "@yarnpkg/cli-dist"
                    }
                    .into(),
                );

                let zpm = match (env::consts::OS, env::consts::ARCH) {
                    ("macos", "aarch64") => Some("aarch64-apple-darwin"),
                    ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
                    ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
                    ("linux", "x86") => Some("i686-unknown-linux-musl"),
                    _ => None,
                }
                .map(|target| {
                    SpecSource::Github(GithubSource {
                        repo: "yarnpkg/zpm",
                        tag_prefix: "v",
                        asset: format!("yarn-{target}.zip"),
                    })
                });

                if self.version.is_below_major(6) {
                    vec![npm]
                } else if self.version.is_exact() {
                    vec![zpm.ok_or_else(|| {
                        eyre!(
                            "Yarn does not provide executables for {:?}",
                            (env::consts::OS, env::consts::ARCH)
                        )
                    })?]
                } else {
                    iter::once(npm).chain(zpm).collect()
                }
            }

            SpecName::Pnpm => vec![SpecSource::Npm("pnpm".into())],

            SpecName::Bun => vec![SpecSource::Github(GithubSource {
                repo: "oven-sh/bun",
                tag_prefix: "bun-v",
                asset: format!(
                    "bun-{}.zip",
                    match (env::consts::OS, env::consts::ARCH) {
                        ("macos", "aarch64") => "darwin-aarch64",
                        ("macos", "x86_64") => "darwin-x64",
                        ("windows", "aarch64") => "windows-aarch64",
                        ("windows", "x86_64") => "windows-x64",
                        ("linux", "aarch64") =>
                            if *util::IS_MUSL {
                                "linux-aarch64-musl"
                            } else {
                                "linux-aarch64"
                            },
                        ("linux", "x86_64") =>
                            if *util::IS_MUSL {
                                "linux-x64-musl"
                            } else {
                                "linux-x64"
                            },
                        (os, arch) => {
                            bail!("Bun does not provide executables for {:?}", (os, arch));
                        }
                    }
                ),
            })],
        })
    }

    /// The source that a resolved version is fetched from, when it differs from the
    /// sources that the specification is resolved through.
    fn fetch_source(&self, version: &semver::Version) -> Result<Option<SpecSource>> {
        Ok(if self.name == SpecName::Pnpm && version.major >= 12 {
            // The platform-specific packages are only discoverable through the
            // `pnpm` package.
            Some(SpecSource::Npm(
                match (env::consts::OS, env::consts::ARCH) {
                    ("macos", "aarch64") => "@pnpm/exe.darwin-arm64",
                    ("macos", "x86_64") => "@pnpm/exe.darwin-x64",
                    ("windows", "aarch64") => "@pnpm/exe.win32-arm64",
                    ("windows", "x86_64") => "@pnpm/exe.win32-x64",
                    ("linux", "aarch64") => {
                        if *util::IS_MUSL {
                            "@pnpm/exe.linux-arm64-musl"
                        } else {
                            "@pnpm/exe.linux-arm64"
                        }
                    }
                    ("linux", "x86_64") => {
                        if *util::IS_MUSL {
                            "@pnpm/exe.linux-x64-musl"
                        } else {
                            "@pnpm/exe.linux-x64"
                        }
                    }
                    (os, arch) => {
                        bail!("pnpm does not provide executables for {:?}", (os, arch));
                    }
                }
                .into(),
            ))
        } else {
            None
        })
    }

    /// The path, relative to a release's unpacked root, of the file whose hash is recorded
    /// as the integrity of the release, when it is not the hash of the release itself.
    ///
    /// This special handling for Yarn is inherited from Corepack. Corepack downloads Yarn
    /// as a file rather than as a package, and calculates the hash from that file. We
    /// download the package, but calculate the hash for the file anyway for the sake of
    /// compatibility.
    pub fn integrity_path<'a>(&self, release: &'a Release) -> Result<Option<&'a str>> {
        if let Release::Npm(version) = release
            && self.name == SpecName::Yarn
        {
            let bin_path = version
                .bin
                .get("yarn")
                .ok_or_else(|| eyre!("could not resolve yarn bin path in {version}"))?;

            Ok(Some(bin_path))
        } else {
            Ok(None)
        }
    }

    pub async fn verify_integrity(
        &self,
        bytes: &[u8],
        unpack_root: &Path,
        resolution: &Resolution,
    ) -> Result<()> {
        let Some(integrity) = self.version.integrity()? else {
            return Ok(());
        };

        if let Some(resolved) = &resolution.resolved {
            // The contents of the resolved release are never downloaded, so the recorded
            // integrity is compared against the one its registry reports.

            let reported = resolved.integrity()?;

            if reported.as_ref() != Some(&integrity) {
                bail!(
                    "integrity (spec) failed to verify for {self} (expected: {integrity}, actual: {})",
                    reported.map_or_else(|| "none".to_owned(), |i| i.to_string())
                );
            }
        } else {
            let bytes = match self.integrity_path(&resolution.release)? {
                Some(path) => Cow::Owned(fs::read(unpack_root.join(path)).await?),
                None => Cow::Borrowed(bytes),
            };

            if let Err((expected, actual)) = integrity.verify(&bytes) {
                bail!(
                    "integrity (spec) failed to verify for {self} (expected: {expected}, actual: {actual})"
                );
            }
        }

        debug!("integrity (spec) verified for {self}");

        Ok(())
    }

    pub async fn resolve(&self) -> Result<Resolution> {
        let mut tasks = task::JoinSet::new();

        for source in self.sources()? {
            let version = self.version.clone();
            tasks.spawn(async move { source.resolve(&version).await });
        }

        let mut resolved: Option<(semver::Version, Release)> = None;
        let mut errors = Vec::new();

        for result in tasks.join_all().await {
            let release = match result {
                Ok(Some(release)) => release,
                Ok(None) => continue,

                // Tolerated as long as another source resolves, so that an outage or a
                // rate limit in one registry is not fatal.
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };

            let version = release.version().parse::<semver::Version>()?;

            if resolved
                .as_ref()
                .is_none_or(|(best, _)| best.cmp_precedence(&version).is_lt())
            {
                resolved = Some((version, release));
            }
        }

        let Some((version, release)) = resolved else {
            return Err(errors.into_iter().next().map_or_else(
                || eyre!("could not find matching version for {self}"),
                |err| err.wrap_err(format!("could not resolve {self}")),
            ));
        };

        for err in errors {
            warn!("a source for {self} failed to resolve: {err}");
        }

        Ok(match self.fetch_source(&version)? {
            Some(source) => Resolution {
                release: source
                    .resolve(&SpecVersion::Exact(version))
                    .await?
                    .ok_or_else(|| eyre!("could not find matching version for {self}"))?,
                resolved: Some(release),
            },
            None => Resolution {
                release,
                resolved: None,
            },
        })
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
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
    Bun,
}

impl SpecName {
    /// The file name of the executable in releases that provide one instead of a package.
    #[must_use]
    pub fn standalone_bin(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            // The `yarn` executable alongside it is a version manager of its own.
            Self::Yarn => "yarn-bin",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
        }
    }
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

    /// Whether this specification is constrained to versions below a major version.
    #[must_use]
    pub fn is_below_major(&self, major: u64) -> bool {
        match self {
            Self::Exact(version) => version.major < major,
            Self::SemverReq(req) => req.comparators.iter().any(|c| match c.op {
                semver::Op::Exact | semver::Op::LessEq | semver::Op::Tilde | semver::Op::Caret => {
                    c.major < major
                }
                semver::Op::Less => {
                    c.major < major
                        || c.major == major
                            && c.minor.is_none_or(|n| n == 0)
                            && c.patch.is_none_or(|n| n == 0)
                }
                _ => false,
            }),
            Self::DistTag(_) => false,
        }
    }

    /// The version without the build metadata that records the integrity, as registry
    /// URLs and release tags do not carry it.
    pub fn to_plain_string(&self) -> String {
        match self {
            Self::Exact(version) => {
                let mut version = version.clone();
                version.build = semver::BuildMetadata::EMPTY;
                version.to_string()
            }
            Self::SemverReq(req) => req.to_string(),
            Self::DistTag(tag) => tag.clone(),
        }
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
        match self {
            Self::Exact(version) => version.fmt(f),
            Self::SemverReq(req) => req.fmt(f),
            Self::DistTag(tag) => f.write_str(tag),
        }
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
pub struct SpecVersionIntegrity {
    algorithm: &'static aws_lc_rs::digest::Algorithm,
    digest: Vec<u8>,
}

static ALGORITHMS: &[(&str, &aws_lc_rs::digest::Algorithm)] = {
    use aws_lc_rs::digest::{SHA1_FOR_LEGACY_USE_ONLY, SHA224, SHA256, SHA384, SHA512};

    &[
        ("sha512", &SHA512),
        ("sha384", &SHA384),
        ("sha256", &SHA256),
        ("sha224", &SHA224),
        ("sha1", &SHA1_FOR_LEGACY_USE_ONLY),
    ]
};

impl SpecVersionIntegrity {
    pub fn sha1(digest: Vec<u8>) -> Self {
        Self {
            algorithm: &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
            digest,
        }
    }

    pub fn sha256(digest: Vec<u8>) -> Self {
        Self {
            algorithm: &aws_lc_rs::digest::SHA256,
            digest,
        }
    }

    pub fn sha512(digest: Vec<u8>) -> Self {
        Self {
            algorithm: &aws_lc_rs::digest::SHA512,
            digest,
        }
    }

    pub fn parse(s: &str) -> Result<Option<Self>> {
        for &(name, algorithm) in ALGORITHMS {
            if let Some(hash) = s.strip_prefix(name).and_then(|rest| rest.strip_prefix('.')) {
                return Ok(Some(Self {
                    algorithm,
                    digest: hex::decode(hash)?,
                }));
            }
        }

        Ok(None)
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<(), (String, String)> {
        use aws_lc_rs::{constant_time::verify_slices_are_equal, digest::digest};

        let expected = &self.digest;
        let actual = digest(self.algorithm, bytes);

        if verify_slices_are_equal(expected, actual.as_ref()).is_ok() {
            Ok(())
        } else {
            Err((hex::encode(expected), hex::encode(actual)))
        }
    }
}

impl fmt::Display for SpecVersionIntegrity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(&(name, _)) = ALGORITHMS
            .iter()
            .find(|&&(_, algorithm)| algorithm == self.algorithm)
        else {
            return Err(fmt::Error);
        };

        write!(f, "{}.{}", name, hex::encode(&self.digest))
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
    Pn,
    Pnx,
    Bun,
    Bunx,
}

impl SpecBin {
    pub fn to_name(self) -> SpecName {
        match self {
            Self::Npm | Self::Npx => SpecName::Npm,
            Self::Yarn | Self::Yarnpkg => SpecName::Yarn,
            Self::Pnpm | Self::Pnpx | Self::Pn | Self::Pnx => SpecName::Pnpm,
            Self::Bun | Self::Bunx => SpecName::Bun,
        }
    }

    /// The arguments that this binary prepends when it is an alias for a subcommand.
    #[must_use]
    pub fn to_args(self) -> &'static [&'static str] {
        match self {
            Self::Npm | Self::Yarn | Self::Yarnpkg | Self::Pnpm | Self::Pn | Self::Bun => &[],
            Self::Npx => &["exec"],
            Self::Pnpx | Self::Pnx => &["dlx"],
            Self::Bunx => &["x"],
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
        impl $enum {
            pub const VARIANTS: &[Self] = &[$(Self::$member),+];
        }

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
    Bun = "bun",
}

impl_fromstr_display! {
    SpecBin,
    Npm = "npm",
    Npx = "npx",
    Yarn = "yarn",
    Yarnpkg = "yarnpkg",
    Pnpm = "pnpm",
    Pnpx = "pnpx",
    Pn = "pn",
    Pnx = "pnx",
    Bun = "bun",
    Bunx = "bunx",
}
