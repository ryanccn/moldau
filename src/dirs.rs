// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::LazyLock};

use etcetera::{AppStrategy, AppStrategyArgs, app_strategy, choose_app_strategy};

#[cfg(not(windows))]
type AppStrategyType = app_strategy::Xdg;
#[cfg(windows)]
type AppStrategyType = app_strategy::Windows;

static STRATEGY: LazyLock<AppStrategyType> = LazyLock::new(|| {
    choose_app_strategy(AppStrategyArgs {
        app_name: "moldau".to_string(),
        ..Default::default()
    })
    .unwrap()
});

pub fn data() -> PathBuf {
    STRATEGY.data_dir()
}

pub fn cache() -> PathBuf {
    STRATEGY.cache_dir()
}
