#!/usr/bin/env bash
# Sets the workspace version to 0.1.N, where N is the number of commits in
# the current history (run on main, this is the number of commits on main).
# Prints the version. Needs the full history (fetch-depth: 0 in Actions).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
n=$(git rev-list --count HEAD)
version="0.1.$n"
# The first `version = ...` line is [workspace.package].
sed -i -E "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
echo "$version"
