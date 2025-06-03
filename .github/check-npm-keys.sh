#! /usr/bin/env nix
#! nix shell nixpkgs#bash nixpkgs#curl nixpkgs#jq nixpkgs#gnugrep --command bash
#  shellcheck shell=bash

# SPDX-FileCopyrightText: 2025 Ryan Cao <hello@ryanccn.dev>
#
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

bad=no

for key in $(curl https://registry.npmjs.org/-/npm/v1/keys | jq -r '.keys | map(.keyid) | join("\n")'); do
    if ! grep -F "$key" "src/models/npm.rs" > /dev/null; then
        echo "key \"$key\" not found in src/models/npm.rs"
        bad=yes
    fi
done

if [ "$bad" = "yes" ]; then
    exit 1
else
    echo "all keys are present in src/models/npm.rs"
fi
