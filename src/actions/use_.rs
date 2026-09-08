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
    models::{Release, Spec, SpecName, SpecVersion, SpecVersionIntegrity},
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
    assert!(spec.version.is_exact());

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

    let resolution = spec.resolve().await?;
    let mut version: semver::Version = resolution.release.version().parse()?;

    let integrity = if let Release::Npm(version_data) = &resolution.release
        && spec.name == SpecName::Yarn
    {
        use aws_lc_rs::digest::{SHA512, digest};

        // Yarn from the npm registry takes its integrity from the hash of the bin file,
        // for compatibility with Corepack.

        let (cache_path, _) = fetch_version(spec, &resolution).await?;

        let bin_path = version_data
            .bin
            .get("yarn")
            .ok_or_else(|| eyre!("could not resolve yarn bin path in {version_data}"))?;

        let bin_contents = fs::read(cache_path.join(bin_path)).await?;

        Some(SpecVersionIntegrity::sha512(
            digest(&SHA512, &bin_contents).as_ref().to_vec(),
        ))
    } else if resolution.pinned().is_platform_specific() {
        // An integrity is left out when it could only be verified on the platform it was
        // recorded on.
        None
    } else {
        resolution.pinned().integrity()?
    };

    if let Some(integrity) = integrity {
        version.build = semver::BuildMetadata::new(&integrity.to_string())?;
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
