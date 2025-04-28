// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod download;
mod exit_code_error;
mod log_display;

use eyre::Result;
use std::{borrow::Cow, path::Path};
use tokio::fs;

pub use download::*;
pub use exit_code_error::*;
pub use log_display::*;

pub async fn find_root(path: &Path) -> Result<Cow<Path>> {
    let mut readdir = fs::read_dir(&path).await?;
    let mut only_entry = None;

    while let Some(entry) = readdir.next_entry().await?.map(|de| de.path()) {
        if only_entry.is_some() || !entry.is_dir() {
            return Ok(Cow::Borrowed(path));
        }

        only_entry.replace(entry);
    }

    match only_entry {
        Some(path) => Ok(Cow::Owned(path)),
        None => Ok(Cow::Borrowed(path)),
    }
}
