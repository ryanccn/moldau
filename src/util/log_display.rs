// SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use owo_colors::{Color, OwoColorize as _};
use std::{
    fmt::{self, Debug, Display},
    marker::PhantomData,
};

pub trait LogDisplay {
    fn log_display<C: Color>(&self) -> LogDisplayThing<&Self, C>;
}

#[repr(transparent)]
pub struct LogDisplayThing<T, C: Color> {
    inner: T,
    _color: PhantomData<C>,
}

impl<T> LogDisplay for T {
    fn log_display<C: Color>(&self) -> LogDisplayThing<&Self, C> {
        LogDisplayThing {
            inner: self,
            _color: PhantomData,
        }
    }
}

macro_rules! impl_fmt_trait {
    ($($trait:ident),* $(,)?) => {
        $(
            impl<T: $trait, C: Color> $trait for LogDisplayThing<T, C> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let backtick = "`".dimmed().to_string();

                    f.write_str(&backtick)?;
                    $trait::fmt(&self.inner.fg::<C>(), f)?;
                    f.write_str(&backtick)?;

                    Ok(())
                }
            }
        )*
    };
}

impl_fmt_trait!(Display, Debug);
