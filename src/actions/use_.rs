// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use tokio::{fs, io};

use eyre::{Result, eyre};
use log::info;
use owo_colors::colors::Blue;
use serde::Serialize;

use crate::{
    actions::fetch_version,
    models::{NpmPackage, NpmVersion, Spec, SpecName, SpecVersion, SpecVersionIntegrity},
    util::LogDisplay as _,
};

fn detect_indent(s: Option<&str>) -> String {
    if let Some(lines) = s.map(|s| s.lines()) {
        for line in lines {
            let mut whitespace_chs: Vec<char> = Vec::new();

            for ch in line.chars() {
                if !ch.is_whitespace() {
                    break;
                }
                whitespace_chs.push(ch);
            }

            if !whitespace_chs.is_empty() {
                return whitespace_chs.into_iter().collect::<String>();
            }
        }
    }

    "  ".to_string()
}

fn detect_eol(s: Option<&str>) -> String {
    if s.is_some_and(|s| s.contains("\r\n")) {
        "\r\n".to_string()
    } else {
        "\n".to_string()
    }
}

async fn write_package_json(spec: &Spec) -> Result<()> {
    assert!(spec.version.exact().is_some());

    let package_json_path = env::current_dir()?.join("package.json");

    let contents = match fs::read_to_string(&package_json_path).await {
        Ok(contents) => {
            if contents.trim().is_empty() {
                None
            } else {
                Some(contents)
            }
        }
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                None
            } else {
                return Err(err.into());
            }
        }
    };

    let (indent, eol) = (
        detect_indent(contents.as_deref()),
        detect_eol(contents.as_deref()),
    );

    let mut data = match contents {
        Some(contents) => serde_json::from_str::<serde_json::Value>(&contents)?,
        None => serde_json::json!({}),
    }
    .as_object()
    .ok_or_else(|| eyre!("package.json is not an object"))
    .cloned()?;

    if let Some(inner) = data
        .get_mut("devEngines")
        .and_then(|v| v.as_object_mut())
        .and_then(|m| m.get_mut("packageManager"))
        .and_then(|v| v.as_object_mut())
    {
        inner.insert("name".to_string(), spec.name.to_string().into());
        inner.insert("version".to_string(), spec.version.to_string().into());
    } else {
        data.insert("packageManager".to_string(), spec.to_string().into());
    }

    let mut writer = Vec::new();
    data.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes()),
    ))?;
    writer.extend(eol.as_bytes());

    fs::write(&package_json_path, writer).await?;

    Ok(())
}

pub async fn use_(spec: &Spec) -> Result<()> {
    info!(
        "resolving versions that match {}",
        spec.log_display::<Blue>()
    );

    let version_data = match &spec.version {
        SpecVersion::Exact(_) => NpmVersion::fetch(spec).await?,

        SpecVersion::SemverReq(req) => NpmPackage::fetch(spec)
            .await?
            .find_version_req(req)
            .ok_or_else(|| eyre!("could not find matching version for {spec}"))?,

        SpecVersion::DistTag(tag) => NpmPackage::fetch(spec)
            .await?
            .find_dist_tag(tag)
            .ok_or_else(|| eyre!("could not find matching version for {spec}"))?,
    };

    let mut version = version_data.version.clone();

    if spec.name == SpecName::Yarn {
        use sha2::{Digest as _, Sha512};

        // If the package manager is Yarn, we fetch the version and set the integrity
        // as the hash of the bin file, according to Corepack's special handling (see
        // `src/actions/fetch.rs` for related details).

        let (cache_path, _) = fetch_version(spec, &version_data).await?;

        let bin_path = version_data
            .bin
            .get("yarn")
            .ok_or_else(|| eyre!("could not resolve yarn bin path in {version_data}"))?;

        let bin_contents = fs::read(cache_path.join(bin_path)).await?;

        let sha512 = Sha512::digest(&bin_contents).to_vec();

        version.build =
            semver::BuildMetadata::new(&SpecVersionIntegrity::SHA512(sha512).to_string())?;
    } else {
        // Otherwise, we set the integrity from data provided by the npm registry.
        version.build = semver::BuildMetadata::new(&version_data.integrity()?.to_string())?;
    }

    let resolved_spec = Spec {
        name: spec.name,
        version: SpecVersion::Exact(version),
    };

    write_package_json(&resolved_spec).await?;
    info!(
        "set package manager to {}",
        resolved_spec.log_display::<Blue>()
    );

    Ok(())
}
