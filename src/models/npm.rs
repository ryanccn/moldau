// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, env, fmt, sync::LazyLock};

use base64::prelude::{BASE64_STANDARD, Engine as _};
use eyre::{Result, bail, eyre};
use log::debug;
use reqwest::{
    Url,
    header::{self, HeaderMap, HeaderValue},
};
use serde::{Deserialize, de::DeserializeOwned};

use super::SpecVersionIntegrity;
use crate::http;

static NPM_REGISTRY: LazyLock<String> = LazyLock::new(|| {
    env::var("COREPACK_NPM_REGISTRY").unwrap_or_else(|_| "https://registry.npmjs.org".to_string())
});

static NPM_INSTALL_HEADER_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

fn npm_auth_header() -> Option<HeaderValue> {
    let credential = if let Ok(token) = env::var("COREPACK_NPM_TOKEN") {
        format!("Bearer {token}")
    } else if let Ok(username) = env::var("COREPACK_NPM_USERNAME")
        && let Ok(password) = env::var("COREPACK_NPM_PASSWORD")
    {
        format!(
            "Basic {}",
            BASE64_STANDARD.encode(format!("{username}:{password}"))
        )
    } else {
        return None;
    };

    http::sensitive_header(credential, "npm registry credentials")
}

static NPM_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut headers = HeaderMap::new();

    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static(NPM_INSTALL_HEADER_ACCEPT),
    );

    if let Some(header) = npm_auth_header() {
        headers.insert(header::AUTHORIZATION, header);
    }

    headers
});

async fn fetch<T: DeserializeOwned>(url: Url) -> Result<Option<T>> {
    http::fetch_json("npm registry", url, &NPM_HEADERS).await
}

fn registry_url(segments: &[&str]) -> Result<Url> {
    let mut url = Url::parse(&NPM_REGISTRY)?;

    url.path_segments_mut()
        .map_err(|()| eyre!("failed to construct npm registry URL"))?
        .extend(segments);

    Ok(url)
}

#[derive(Deserialize, Clone, Debug)]
pub struct NpmPackage {
    pub versions: HashMap<String, NpmVersion>,
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
        write!(f, "npm:{}@{}", self.name, self.version)
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
    pub async fn fetch(package: &str) -> Result<Self> {
        fetch(registry_url(&[package])?)
            .await?
            .ok_or_else(|| eyre!("{package} does not exist in the npm registry"))
    }

    #[must_use]
    pub fn find_version_req(&self, req: &semver::VersionReq) -> Option<NpmVersion> {
        self.versions
            .iter()
            .filter_map(|(k, v)| semver::Version::parse(k).ok().map(|s| (s, v)))
            .filter(|(k, _)| req.matches(k))
            .max_by(|a, b| a.0.cmp_precedence(&b.0))
            .map(|(_, version)| version.clone())
    }
}

impl NpmVersion {
    pub async fn fetch(package: &str, version: &str) -> Result<Option<Self>> {
        fetch(registry_url(&[package, version])?).await
    }

    pub fn integrity(&self) -> Result<SpecVersionIntegrity> {
        if let Some(integrity) = &self.dist.integrity {
            let sha512 = BASE64_STANDARD.decode(
                integrity
                    .strip_prefix("sha512-")
                    .ok_or_else(|| eyre!("unexpected format in npm integrity: {integrity:?}"))?,
            )?;

            Ok(SpecVersionIntegrity::sha512(sha512))
        } else {
            Ok(SpecVersionIntegrity::sha1(hex::decode(&self.dist.shasum)?))
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
        use aws_lc_rs::signature::{ECDSA_P256_SHA256_ASN1, ParsedPublicKey};

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
                let p256_message = format!(
                    "{}@{}:{}",
                    self.name,
                    self.version,
                    self.dist.integrity.as_deref().unwrap_or_default()
                );

                let p256_public_key = ParsedPublicKey::new(
                    &ECDSA_P256_SHA256_ASN1,
                    &BASE64_STANDARD.decode(public_key.key)?,
                )?;

                let p256_signature = BASE64_STANDARD.decode(&signature.sig)?;

                if let Err(err) =
                    p256_public_key.verify_sig(p256_message.as_bytes(), &p256_signature)
                {
                    bail!("ECDSA signature failed to verify for {self}: {err}");
                }

                debug!(
                    "ECDSA signature verified for {self} (keyid: {})",
                    public_key.keyid
                );
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
