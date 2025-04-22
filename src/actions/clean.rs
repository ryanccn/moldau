// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;
use tokio::fs;

use eyre::Result;
use log::info;
use owo_colors::{OwoColorize as _, colors::Blue};

use crate::{dirs, models::SpecName, util::LogDisplay as _};

pub async fn clean(all: bool) -> Result<()> {
    let all_versions_path = dirs::cache().join("versions");

    for name in SpecName::VARIANTS {
        let mut cached_versions: BTreeSet<semver::Version> = BTreeSet::new();
        let versions_path = all_versions_path.join(name.to_string());

        if let Ok(mut read_dir) = fs::read_dir(&versions_path).await {
            while let Ok(Some(entry)) = read_dir.next_entry().await {
                if let Ok(version) = semver::Version::parse(&entry.file_name().to_string_lossy()) {
                    cached_versions.insert(version);
                }
            }
        }

        if !all {
            cached_versions.pop_last();
        }

        for version in &cached_versions {
            fs::remove_dir_all(versions_path.join(version.to_string())).await?;
        }

        info!(
            "removed {} versions of {}{}",
            cached_versions.len().green(),
            name.log_display::<Blue>(),
            if all {
                " (including latest)".dimmed().to_string()
            } else {
                String::new()
            }
        );
    }

    Ok(())
}
