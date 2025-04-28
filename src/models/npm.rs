// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, env, fmt, sync::LazyLock};

use base64::prelude::{BASE64_STANDARD, Engine as _};
use eyre::{Result, bail, eyre};
use log::debug;
use reqwest::{Url, header};
use serde::Deserialize;

use super::{Spec, SpecVersionIntegrity};
use crate::http::HTTP;

static NPM_REGISTRY: LazyLock<String> = LazyLock::new(|| {
    env::var("COREPACK_NPM_REGISTRY").unwrap_or_else(|_| "https://registry.npmjs.org".to_string())
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
    pub version: String,
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
    #[serde(default)]
    pub signatures: Vec<NpmVersionSignature>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct NpmVersionSignature {
    pub keyid: String,
    pub sig: String,
}

impl NpmPackage {
    pub async fn fetch(spec: &Spec) -> Result<Self> {
        let mut url = Url::parse(&NPM_REGISTRY)?;
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
        let mut url = Url::parse(&NPM_REGISTRY)?;
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

    pub fn verify_integrity(&self, bytes: &[u8]) -> Result<()> {
        if let Err((expected, actual)) = self.integrity()?.verify(bytes) {
            bail!(
                "integrity (download) failed to verify for {self} (expected: {expected}, actual: {actual})"
            );
        }

        debug!("integrity (download) verified for {self}");
        Ok(())
    }

    pub fn verify_signature(&self) -> Result<()> {
        use base64::prelude::{BASE64_STANDARD, Engine as _};
        use p256::{
            ecdsa::{Signature, VerifyingKey, signature::Verifier as _},
            pkcs8::DecodePublicKey,
        };

        if !Url::parse(NPM_REGISTRY.as_str()).is_ok_and(|url| {
            url.domain()
                .is_some_and(|domain| domain == "registry.npmjs.org")
        }) {
            debug!("skipped ECDSA signature verification for {self} (not `registry.npmjs.org`)");
            return Ok(());
        }

        for signature in &self.dist.signatures {
            if let Some(public_key) = NPM_REGISTRY_PUBLIC_KEYS
                .iter()
                .find(|key| key.keyid == signature.keyid)
            {
                let name_b = self.name.as_bytes();
                let version_b = self.version.as_bytes();
                let integrity_b = self
                    .dist
                    .integrity
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes();

                let mut p256_message = Vec::with_capacity(
                    name_b
                        .len()
                        .saturating_add(version_b.len())
                        .saturating_add(integrity_b.len())
                        .saturating_add(2),
                );

                p256_message.extend_from_slice(name_b);
                p256_message.extend_from_slice(b"@");
                p256_message.extend_from_slice(version_b);
                p256_message.extend_from_slice(b":");
                p256_message.extend_from_slice(integrity_b);

                let p256_public_key =
                    VerifyingKey::from_public_key_der(&BASE64_STANDARD.decode(public_key.key)?)?;

                let p256_signature = Signature::from_der(&BASE64_STANDARD.decode(&signature.sig)?)?;

                if let Err(err) = p256_public_key.verify(&p256_message, &p256_signature) {
                    bail!("ECDSA signature failed to verify for {self}: {err}");
                } else {
                    debug!(
                        "ECDSA signature verified for {self} (keyid: {})",
                        public_key.keyid
                    );
                }
            }
        }

        Ok(())
    }
}

pub struct NpmRegistryPublicKey {
    pub keyid: &'static str,
    pub key: &'static str,
}

// https://registry.npmjs.org/-/npm/v1/keys
pub static NPM_REGISTRY_PUBLIC_KEYS: [&NpmRegistryPublicKey; 2] = [
    &NpmRegistryPublicKey {
        keyid: "SHA256:jl3bwswu80PjjokCgh0o2w5c2U4LhQAE57gj9cz1kzA",
        key: "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE1Olb3zMAFFxXKHiIkQO5cJ3Yhl5i6UPp+IhuteBJbuHcA5UogKo0EWtlWwW6KSaKoTNEYL7JlCQiVnkhBktUgg==",
    },
    &NpmRegistryPublicKey {
        keyid: "SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U",
        key: "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEY6Ya7W++7aUPzvMTrezH6Ycx3c+HOKYCcNGybJZSCJq/fd7Qa8uuAKtdIkUQtQiEKERhAmE5lMMJhP8OkDOa2g==",
    },
];
