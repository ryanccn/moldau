// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{error::Error, fmt, process::ExitCode as StdExitCode};

#[derive(Debug)]
pub struct ExitCode(pub StdExitCode);

impl ExitCode {
    pub const SUCCESS: Self = Self(StdExitCode::SUCCESS);
    pub const FAILURE: Self = Self(StdExitCode::FAILURE);
}

impl From<u8> for ExitCode {
    fn from(value: u8) -> Self {
        Self(StdExitCode::from(value))
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Error for ExitCode {}
