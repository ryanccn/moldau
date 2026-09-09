// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod clean;
mod exec;
mod fetch;
mod prepare;
mod shims;
mod use_;

pub use clean::clean;
pub use exec::exec;
pub use fetch::{fetch_spec, fetch_version};
pub use prepare::prepare;
pub use shims::shims;
pub use use_::use_;

use std::collections::BTreeSet;
use tokio::fs;

use eyre::Result;

use crate::{dirs, models::SpecName};

async fn cached_versions(name: SpecName) -> Result<BTreeSet<semver::Version>> {
    let mut versions = BTreeSet::new();

    if let Ok(mut read_dir) = fs::read_dir(dirs::versions(name)).await {
        while let Some(entry) = read_dir.next_entry().await? {
            if let Ok(version) = semver::Version::parse(&entry.file_name().to_string_lossy()) {
                versions.insert(version);
            }
        }
    }

    Ok(versions)
}
