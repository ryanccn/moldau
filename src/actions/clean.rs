// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeSet, time::Duration};
use tokio::fs;

use eyre::Result;
use log::{debug, info};
use owo_colors::{OwoColorize as _, colors::Blue};

use crate::{dirs, models::SpecName, util::LogDisplay as _};

/// Younger temporary directories may belong to a fetch running in another process.
static STALE_AFTER: Duration = Duration::from_hours(1);

async fn clean_temp() -> Result<usize> {
    let Ok(mut read_dir) = fs::read_dir(dirs::cache()).await else {
        return Ok(0);
    };

    let mut removed = 0;

    while let Some(entry) = read_dir.next_entry().await? {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(dirs::TEMP_PREFIX)
        {
            continue;
        }

        let stale = entry.metadata().await.ok().is_none_or(|meta| {
            meta.modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_none_or(|age| age > STALE_AFTER)
        });

        if !stale {
            continue;
        }

        let path = entry.path();
        fs::remove_dir_all(&path).await?;
        debug!("removed stale temporary directory {}", path.display());
        removed += 1;
    }

    Ok(removed)
}

pub async fn clean(all: bool) -> Result<()> {
    let all_versions_path = dirs::cache().join("versions");

    for name in SpecName::VARIANTS {
        let mut cached_versions: BTreeSet<semver::Version> = BTreeSet::new();
        let versions_path = all_versions_path.join(name.to_string());

        if let Ok(mut read_dir) = fs::read_dir(&versions_path).await {
            while let Some(entry) = read_dir.next_entry().await? {
                if let Ok(version) = semver::Version::parse(&entry.file_name().to_string_lossy()) {
                    cached_versions.insert(version);
                }
            }
        }

        if !all {
            cached_versions.pop_last();
        }

        for version in &cached_versions {
            let path = versions_path.join(version.to_string());
            fs::remove_dir_all(&path).await?;
            debug!("removed {version} -> {}", path.display());
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

    let removed_temp = clean_temp().await?;

    if removed_temp > 0 {
        info!(
            "removed {} stale temporary {}",
            removed_temp.green(),
            if removed_temp == 1 {
                "directory"
            } else {
                "directories"
            }
        );
    }

    Ok(())
}
