#!/usr/bin/env bash
#
# Build a lean docs bundle: only what the goose-doc-guide skill reads.
#
# The skill reads `goose-docs-map.md` and then only the paths named in it. The
# full site bundle also carries blog images and videos (hundreds of megabytes)
# that are needed only for browsing the HTML site in a browser, so they are
# excluded here.
#
# The result is small enough to embed in the binary, which is what makes a
# single executable sufficient.
#
# Usage: build-lean-bundle.sh <goose-version> --build-dir DIR --out FILE
#        build-lean-bundle.sh <goose-version>            # build the site first
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

VERSION=""
BUILD_DIR=""
OUT=""
WORK_DIR=""

usage() {
  cat <<'EOF'
Usage: build-lean-bundle.sh <goose-version> [options]

  <goose-version>   goose release tag or version (e.g. v1.52.0 or 1.52.0)

Options:
  --build-dir DIR   Existing site build to extract from (skips the site build)
  --out FILE        Output tarball (default: <version>-lean.tar.gz)
  --work-dir DIR    Working directory for the goose checkout
  -h, --help        Show this help
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --build-dir) BUILD_DIR="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --work-dir) WORK_DIR="$2"; shift 2 ;;
    -*) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    *) VERSION="$1"; shift ;;
  esac
done

if [ -z "$VERSION" ]; then
  echo "Error: goose version is required" >&2
  usage >&2
  exit 2
fi

case "$VERSION" in
  v*) BARE="${VERSION#v}" ;;
  *) BARE="$VERSION" ;;
esac

[ -n "$OUT" ] || OUT="$ROOT/out/goose-docs-$BARE-lean.tar.gz"

# Build the site when no build directory was supplied.
if [ -z "$BUILD_DIR" ]; then
  WORK="$(mktemp -d)"
  trap 'rm -rf "$WORK"' EXIT
  "$ROOT/tools/build-docs-bundle.sh" "$BARE" \
    --out-dir "$WORK/out" ${WORK_DIR:+--work-dir "$WORK_DIR"} >/dev/null
  # Extract the site bundle rather than rebuilding.
  mkdir -p "$WORK/site"
  tar xzf "$WORK/out/goose-docs-$BARE.tar.gz" -C "$WORK/site"
  BUILD_DIR="$WORK/site"
fi

if [ ! -f "$BUILD_DIR/goose-docs-map.md" ]; then
  echo "Error: $BUILD_DIR is not a docs root" >&2
  exit 1
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp "$BUILD_DIR/goose-docs-map.md" "$STAGE/"

# Copy exactly the pages the map names, rejecting anything that would escape.
COUNT=0
while read -r rel; do
  [ -n "$rel" ] || continue
  case "$rel" in
    /*|*..*) echo "Error: unsafe map entry: $rel" >&2; exit 1 ;;
  esac
  src="$BUILD_DIR/$rel"
  if [ ! -f "$src" ]; then
    echo "Error: map entry missing from build: $rel" >&2
    exit 1
  fi
  mkdir -p "$STAGE/$(dirname "$rel")"
  cp "$src" "$STAGE/$rel"
  COUNT=$((COUNT + 1))
done <<EOF
$(grep -oE '\(docs/[^)]+\.md\)' "$BUILD_DIR/goose-docs-map.md" | tr -d '()' | sort -u)
EOF

if [ "$COUNT" -eq 0 ]; then
  echo "Error: the map lists no pages" >&2
  exit 1
fi

# Record what this is, so `goose-doc doctor` can report a version for the
# embedded bundle too.
cat > "$STAGE/manifest.json" <<EOF
{
  "goose_version": "$BARE",
  "variant": "lean",
  "entries": $COUNT,
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

mkdir -p "$(dirname "$OUT")"
tar czf "$OUT" -C "$STAGE" .

echo "lean bundle: $OUT"
echo "  pages:  $COUNT"
echo "  size:   $(du -h "$OUT" | cut -f1)"
