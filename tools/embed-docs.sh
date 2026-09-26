#!/usr/bin/env bash
#
# Place a docs bundle where `include_dir!` picks it up, so the next
# `cargo build` embeds it and the binary works on its own.
#
# The embedded bundle is what makes a single executable sufficient: the server
# starts and serves the documentation with no download. A downloaded site bundle
# (hundreds of megabytes, including blog images and videos) remains available
# for browsing the full HTML site, and overrides the embedded copy.
#
# Usage: embed-docs.sh <bundle.tar.gz> [<version>]
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${1:-}"
VERSION="${2:-}"

if [ -z "$BUNDLE" ]; then
  echo "Usage: embed-docs.sh <bundle.tar.gz> [<version>]" >&2
  exit 2
fi

if [ ! -f "$BUNDLE" ]; then
  echo "Error: bundle not found: $BUNDLE" >&2
  exit 1
fi

# Refuse the full site bundle. It carries blog images and videos that nothing
# reads, and embedding it would produce a multi-hundred-megabyte binary. The
# browsable bundle (the site minus that media) is well under this limit.
MAX_BYTES=$((64 * 1024 * 1024))
SIZE=$(wc -c < "$BUNDLE" | tr -d ' ')
if [ "$SIZE" -gt "$MAX_BYTES" ]; then
  echo "Error: $BUNDLE is $((SIZE / 1048576)) MB." >&2
  echo "       Only the lean bundle belongs in the binary (see build-lean-bundle.sh)." >&2
  exit 1
fi

if [ -z "$VERSION" ]; then
  # Bundle names carry a variant suffix: goose-docs-1.52.0-lean.tar.gz,
  # goose-docs-1.52.0-browsable.tar.gz, or goose-docs-1.52.0.tar.gz.
  VERSION="$(basename "$BUNDLE" | sed -n 's/^goose-docs-\(.*\)\.tar\.gz$/\1/p' | sed 's/-lean$//' | sed 's/-browsable$//')"
fi

if [ -z "$VERSION" ]; then
  echo "Error: could not infer a version from $BUNDLE; pass it explicitly" >&2
  exit 1
fi

DEST="$ROOT/src/embedded"
rm -rf "$DEST"
mkdir -p "$DEST"

tar xzf "$BUNDLE" -C "$DEST"

# Validate before writing anything into the source tree: a broken bundle would
# otherwise be discovered only at runtime.
if [ ! -f "$DEST/goose-docs-map.md" ]; then
  echo "Error: bundle has no goose-docs-map.md" >&2
  rm -rf "$DEST"
  exit 1
fi

if [ ! -d "$DEST/docs" ]; then
  echo "Error: bundle has no docs/ directory" >&2
  rm -rf "$DEST"
  exit 1
fi

# Every path the map names must be present, since the skill reads only those.
MISSING=0
COUNT=0
while read -r rel; do
  [ -n "$rel" ] || continue
  COUNT=$((COUNT + 1))
  [ -f "$DEST/$rel" ] || { echo "  missing: $rel" >&2; MISSING=$((MISSING + 1)); }
done <<EOF
$(grep -oE '\(docs/[^)]+\.md\)' "$DEST/goose-docs-map.md" | tr -d '()' | sort -u)
EOF

if [ "$MISSING" -gt 0 ]; then
  echo "Error: $MISSING of $COUNT map entries missing from the bundle" >&2
  rm -rf "$DEST"
  exit 1
fi

# A manifest makes the version discoverable at runtime; write one when the
# bundle did not carry it.
if [ ! -f "$DEST/manifest.json" ]; then
  cat > "$DEST/manifest.json" <<EOF
{
  "goose_version": "$VERSION",
  "variant": "lean",
  "entries": $COUNT,
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
fi

echo "embedded docs: $VERSION ($COUNT pages) -> $DEST"
du -sh "$DEST"
