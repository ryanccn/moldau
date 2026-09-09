// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{sync::LazyLock, time::Duration};

use eyre::Result;
use log::{debug, warn};
use reqwest::{
    Client, StatusCode, Url,
    header::{HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub static HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .https_only(true)
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .unwrap()
});

pub fn sensitive_header(value: String, what: &str) -> Option<HeaderValue> {
    if let Ok(mut header) = HeaderValue::try_from(value) {
        header.set_sensitive(true);
        Some(header)
    } else {
        warn!("{what} is not a valid header value, ignoring it");
        None
    }
}

pub async fn fetch_json<T: DeserializeOwned>(
    what: &str,
    url: Url,
    headers: &HeaderMap,
) -> Result<Option<T>> {
    debug!("fetching {what}: {url}");

    let resp = HTTP.get(url).headers(headers.clone()).send().await?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }

    Ok(Some(resp.error_for_status()?.json().await?))
}
