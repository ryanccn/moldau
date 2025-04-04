// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eyre::Result;
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};

use crate::http::HTTP;

static PROGRESS_CHAR: &str = "━━";

pub async fn download(prefix: &str, url: impl reqwest::IntoUrl) -> Result<Vec<u8>> {
    let mut resp = HTTP.get(url).send().await?.error_for_status()?;
    let content_length = resp.content_length().unwrap_or_default();

    let mut bytes: Vec<u8> = Vec::with_capacity(content_length.try_into()?);

    let bar = ProgressBar::new(content_length)
        .with_prefix(prefix.to_owned())
        .with_style(
            ProgressStyle::with_template(
                r"       {prefix:.cyan}  {bar:30.cyan/dim}  {decimal_bytes}/{decimal_total_bytes}  {decimal_bytes_per_sec:.dim}",
            )?
            .progress_chars(PROGRESS_CHAR)
        )
        .with_finish(ProgressFinish::Abandon);

    while let Some(chunk) = resp.chunk().await? {
        bytes.extend_from_slice(&chunk);
        bar.inc(chunk.len() as u64);
    }

    bar.set_style(
        ProgressStyle::with_template(
            r"       {prefix:.green}  {bar:30.green}  {decimal_bytes}/{decimal_total_bytes}  {decimal_bytes_per_sec:.dim}"
        )?
        .progress_chars(PROGRESS_CHAR)
    );

    bar.tick();

    Ok(bytes)
}
