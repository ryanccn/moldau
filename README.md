<!--
SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Moldau

Moldau is a modern version manager for JavaScript package managers (npm, Yarn, pnpm, and Bun). It is mostly compatible with Corepack with regards to configuration (supporting both `packageManager` and `devEngines.packageManager`), and provides a CLI that has a better user experience and design.

## Installation

You can install Moldau from the included Nix flake, [crates.io](https://crates.io/crates/moldau), or [GitHub Releases](https://github.com/ryanccn/moldau/releases).

```bash
cargo binstall moldau
```

### Shims

Moldau requires shims to be installed so that it can handle calls to npm, Yarn, pnpm, and Bun. Run `moldau shims` to install shims to the default path, or `moldau shims <dest>` to install them to a specific directory. Then, add the directory containing the shims to the front of your `PATH` so that it takes precedence over other possible installations.

## Usage

```bash
moldau use pnpm@latest
moldau up
moldau prefetch yarn
moldau clean
```

## Sources

Most package managers are resolved and downloaded from the npm registry. Yarn 6 and above and Bun are distributed as standalone executables through GitHub Releases instead, and are resolved and downloaded from there. Moldau reads the `GITHUB_TOKEN` environment variable if it is set, which raises the GitHub API rate limit.

## Corepack compatibility

Moldau aims to be as compatible with Corepack as possible. That being said, it intentionally does not support certain features such as auto pin. Moldau reads the `COREPACK_ENABLE_STRICT`, `COREPACK_NPM_REGISTRY`, `COREPACK_NPM_TOKEN`, `COREPACK_NPM_USERNAME`, and `COREPACK_NPM_PASSWORD` environment variables and interprets them in [the same way that Corepack does](https://github.com/nodejs/corepack#environment-variables).

The integrity that Moldau records in `packageManager` is the one Corepack records, for every package manager Corepack supports. pnpm 12 and above are run from a platform-specific executable rather than through Node.js, but are still pinned by the integrity of the platform-independent package, as Corepack pins them. Yarn 6 and above and Bun are pinned without an integrity, since they are only distributed as platform-specific executables and no integrity for them can be verified from another platform.
