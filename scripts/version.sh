#!/usr/bin/env bash
# Prints the version: 0.1.N, where N is the number of commits on main.
# Builds pass it to the code as UMPL_VERSION (and to cargo-deb), so the
# source tree is never modified. Needs full history in CI (fetch-depth: 0).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
for ref in main origin/main HEAD; do
    if n=$(git rev-list --count "$ref" 2>/dev/null); then
        echo "0.1.$n"
        exit 0
    fi
done
echo "0.1.0"
