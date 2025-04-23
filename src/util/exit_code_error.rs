// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{error::Error, fmt, process::ExitCode};

#[derive(Debug)]
pub struct ExitCodeError(pub ExitCode);

impl ExitCodeError {
    pub const SUCCESS: Self = Self(ExitCode::SUCCESS);
    pub const FAILURE: Self = Self(ExitCode::FAILURE);
}

impl From<u8> for ExitCodeError {
    fn from(value: u8) -> Self {
        Self(ExitCode::from(value))
    }
}

impl fmt::Display for ExitCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Error for ExitCodeError {}

pub trait ToExitCode {
    fn to_exit_code(&self) -> ExitCode;
}

impl<T> ToExitCode for Result<T, eyre::Report> {
    fn to_exit_code(&self) -> ExitCode {
        match self {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                if let Some(code) = err.downcast_ref::<ExitCodeError>() {
                    code.0
                } else {
                    anstream::eprint!("Error: {err:?}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
