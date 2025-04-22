// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::LazyLock;

use reqwest::Client;

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub static HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .https_only(true)
        .user_agent(USER_AGENT)
        .build()
        .unwrap()
});
