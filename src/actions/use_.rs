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
    models::{Spec, SpecVersion, SpecVersionIntegrity},
    util::LogDisplay as _,
};

fn detect_indent(s: Option<&str>) -> String {
    s.and_then(|s| {
        s.lines().find_map(|line| {
            let indent = &line[..line.len() - line.trim_start().len()];
            (!indent.is_empty()).then(|| indent.to_owned())
        })
    })
    .unwrap_or_else(|| "  ".to_string())
}

fn detect_eol(s: Option<&str>) -> &'static str {
    if s.is_some_and(|s| s.contains("\r\n")) {
        "\r\n"
    } else {
        "\n"
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

    let mut value = match contents {
        Some(contents) => serde_json::from_str::<serde_json::Value>(&contents)?,
        None => serde_json::json!({}),
    };

    let data = value
        .as_object_mut()
        .ok_or_else(|| eyre!("package.json is not an object"))?;

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

    // The formatter always writes LF, so CRLF has to be restored afterwards. Line breaks
    // inside strings are escaped, so they are unaffected.
    let mut contents = String::from_utf8(writer)?;
    if eol == "\r\n" {
        contents = contents.replace('\n', eol);
    }
    contents.push_str(eol);

    fs::write(&package_json_path, contents).await?;

    Ok(())
}

pub async fn use_(spec: &Spec) -> Result<()> {
    info!(
        "resolving versions that match {}",
        spec.log_display::<Blue>()
    );

    let resolution = spec.resolve().await?;
    let mut version: semver::Version = resolution.release.version().parse()?;

    let integrity = if let Some(bin_path) = spec.integrity_path(&resolution.release)? {
        use aws_lc_rs::digest::{SHA512, digest};

        // The hash can only be calculated from the unpacked contents of the release.
        let (cache_path, _) = fetch_version(spec, &resolution).await?;
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
