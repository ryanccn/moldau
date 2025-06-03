<!--
SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Moldau

Moldau is a modern version manager for Node.js package managers (npm, Yarn, and pnpm). It is mostly compatible with Corepack with regards to configuration (supporting both `packageManager` and `devEngines.packageManager`), and provides a CLI that has a better user experience and design.

## Installation

You can install Moldau from the included Nix flake, [crates.io](https://crates.io/crates/moldau), or [GitHub Releases](https://github.com/ryanccn/moldau/releases).

```bash
cargo binstall moldau
```

### Shims

Moldau requires shims to be installed so that it can handle calls to npm, Yarn, and pnpm. Run `moldau shims` to install shims to the default path, or `moldau shims <dest>` to install them to a specific directory. Then, add the directory containing the shims to the front of your `PATH` so that it takes precedence over other possible installations.

## Usage

```bash
moldau use pnpm@latest
moldau up
moldau prefetch yarn
moldau clean
```

## Corepack compatibility

Moldau aims to be as compatible with Corepack as possible. That being said, it intentionally does not support certain features such as auto pin. Moldau reads the `COREPACK_ENABLE_STRICT`, `COREPACK_NPM_REGISTRY`, `COREPACK_NPM_TOKEN`, `COREPACK_NPM_USERNAME`, and `COREPACK_NPM_PASSWORD` environment variables and interprets them in [the same way that Corepack does](https://github.com/nodejs/corepack#environment-variables).

Moldau currently does not support Yarn 2.x versions other than 2.4.1. This is due to an internal implementation detail. It does support other versions of Yarn, including Yarn 4 and Yarn 1 (classic).
