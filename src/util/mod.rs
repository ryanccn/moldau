// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod download;
mod exit_code_error;
mod log_display;

use eyre::Result;
use std::{borrow::Cow, env, path::Path, process::Command, sync::LazyLock};
use tokio::fs;

pub use download::*;
pub use exit_code_error::*;
pub use log_display::*;

pub async fn find_root(path: &Path) -> Result<Cow<'_, Path>> {
    let mut read_dir = fs::read_dir(&path).await?;
    let mut only_entry = None;

    while let Some(entry) = read_dir.next_entry().await?.map(|de| de.path()) {
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

pub static IS_MUSL: LazyLock<bool> = LazyLock::new(|| {
    if env::consts::OS == "linux"
        && let Ok(output) = Command::new("ldd").arg("--version").output()
    {
        String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("musl")
    } else {
        false
    }
});
