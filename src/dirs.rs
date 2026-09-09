// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, path::PathBuf, sync::LazyLock};

use crate::models::SpecName;

pub static APP_NAME: &str = "moldau";
pub static TEMP_PREFIX: &str = "moldau-tmp";

static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| env::home_dir().expect("locate the home directory"));

#[cfg(not(windows))]
fn xdg_dir(var: &str, default: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| HOME.join(default))
        .join(APP_NAME)
}

#[cfg(windows)]
fn appdata_dir(var: &str, default: &str, kind: &str) -> PathBuf {
    env::var_os(var)
        .filter(|value| !value.is_empty())
        .map_or_else(|| HOME.join("AppData").join(default), PathBuf::from)
        .join(APP_NAME)
        .join(kind)
}

#[cfg(not(windows))]
pub fn data() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

#[cfg(not(windows))]
pub fn cache() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

#[cfg(windows)]
pub fn data() -> PathBuf {
    appdata_dir("APPDATA", "Roaming", "data")
}

#[cfg(windows)]
pub fn cache() -> PathBuf {
    appdata_dir("LOCALAPPDATA", "Local", "cache")
}

pub fn versions(name: SpecName) -> PathBuf {
    cache().join("versions").join(name.to_string())
}
