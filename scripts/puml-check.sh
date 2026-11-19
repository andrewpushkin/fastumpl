#!/usr/bin/env bash
# Asks PlantUML itself what it makes of every diagram file, so the PlantUML
# corpus test (crates/umpl-draw/tests/puml_corpus.rs) can tell a file that
# is broken from a file fastumpl reads wrongly. Runs inside the dev image:
#
#   scripts/dev.sh scripts/puml-check.sh [folder...]
#
# Folders default to /opt/puml-samples and target/puml-corpus. Writes
# target/puml-check.tsv, one line per diagram (see scripts/PumlCheck.java):
# path, diagram number, kind, ok/error, error line, description or error.
# PlantUML only reads the files; nothing is drawn. Includes from the web
# are fetched, as PlantUML does for its users.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
[ $# -gt 0 ] || set -- /opt/puml-samples target/puml-corpus
OUT=target/puml-check.tsv
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Files that hold a diagram; included-only files are read through the
# files that include them.
find "$@" -type d -name .git -prune -o -type f \
    \( -iname '*.puml' -o -iname '*.plantuml' -o -iname '*.pu' -o -iname '*.wsd' -o -iname '*.iuml' \) \
    -print0 | xargs -0 grep -l '^[[:space:]]*@start' > "$TMP/files" || true
echo "$(wc -l < "$TMP/files") files with diagrams"

java -DPLANTUML_SECURITY_PROFILE=INTERNET -Djava.awt.headless=true \
    -cp /opt/plantuml/plantuml.jar \
    scripts/PumlCheck.java "$TMP/files" "$OUT"
echo "$(cut -f1 "$OUT" | sort -u | wc -l) files, $(grep -c $'\terror\t' "$OUT") diagrams with errors -> $OUT"
